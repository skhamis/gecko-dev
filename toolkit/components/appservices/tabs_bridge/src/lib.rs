/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#![allow(non_snake_case)]

//! This crate bridges the WebExtension storage area interfaces in Firefox
//! Desktop to the extension storage Rust component in Application Services.
//!
//! ## How are the WebExtension storage APIs implemented in Firefox?
//!
//! There are three storage APIs available for WebExtensions:
//! `storage.local`, which is stored locally in an IndexedDB database and never
//! synced to other devices, `storage.sync`, which is stored in a local SQLite
//! database and synced to all devices signed in to the same Firefox Account,
//! and `storage.managed`, which is provisioned in a native manifest and
//! read-only.
//!
//! * `storage.local` is implemented in `ExtensionStorageIDB.jsm`.
//! * `storage.sync` is implemented in a Rust component, `webext_storage`. This
//!   Rust component is vendored in m-c, and exposed to JavaScript via an XPCOM
//!   API in `webext_storage_bridge` (this crate). Eventually, we'll change
//!   `ExtensionStorageSync.jsm` to call the XPCOM API instead of using the
//!   old Kinto storage adapter.
//! * `storage.managed` is implemented directly in `parent/ext-storage.js`.
//!
//! `webext_storage_bridge` implements the `mozIExtensionStorageArea`
//! (and, eventually, `mozIBridgedSyncEngine`) interface for `storage.sync`. The
//! implementation is in `area::StorageSyncArea`, and is backed by the
//! `webext_storage` component.

#[macro_use]
extern crate cstr;
#[macro_use]
extern crate xpcom;

mod error;
mod store;
//mod punt;

use crate::error::{Error, Result};
use golden_gate::{ApplyTask, FerryTask};
use moz_task::{self, DispatchOptions, TaskRunnable};
use nserror::{nsresult, NS_OK};
use nsstring::{nsACString, nsCString, nsString};
use parking_lot::Mutex;
use std::{
    cell::{Ref, RefCell},
    convert::TryInto,
    ffi::OsString,
    mem,
    path::Path,
    path::PathBuf,
    str,
    sync::Arc,
};
use tabs::{TabsEngine, TabsStore};
use thin_vec::ThinVec;
use xpcom::{
    interfaces::{
        mozIBridgedSyncEngineApplyCallback, mozIBridgedSyncEngineCallback,
        mozIExtensionStorageCallback, mozIServicesLogSink, nsIFile, nsISerialEventTarget,
    },
    RefPtr,
};

/// An XPCOM component class for the Rust extension storage API. This class
/// implements the interfaces needed for syncing and storage.
///
/// This class can be created on any thread, but must not be shared between
/// threads. In Rust terms, it's `Send`, but not `Sync`.
#[derive(xpcom)]
#[xpimplements(mozIBridgedSyncEngine)]
#[refcnt = "nonatomic"]
pub struct InitTabsBridge {
    /// A background task queue, used to run all our storage operations on a
    /// thread pool. Using a serial event target here means that all operations
    /// will execute sequentially.
    queue: RefPtr<nsISerialEventTarget>,
    /// The store is lazily initialized on the task queue the first time it's
    /// used.
    //store: RefCell<Option<Arc<TabsStore>>>,
    store: RefCell<Option<Arc<Mutex<TabsEngine>>>>,
}

impl TabsBridge {
    /// Creates a storage area and its task queue.
    pub fn new(db_path: impl AsRef<Path>) -> Result<RefPtr<TabsBridge>> {
        let queue = moz_task::create_background_task_queue(cstr!("TabsBridge"))?;
        //TODO
        let engine = TabsEngine::new(Arc::new(TabsStore::new(db_path)));
        Ok(TabsBridge::allocate(InitTabsBridge {
            queue,
            store: RefCell::new(Some(Arc::new(Mutex::new(engine)))),
        }))
    }

    /// Returns the store for this area, or an error if it's been torn down.
    fn store(&self) -> Result<Ref<'_, Arc<TabsEngine>>> {
        let maybe_store = self.store.borrow();
        if maybe_store.is_some() {
            Ok(Ref::map(maybe_store, |s| s.as_ref().unwrap().lock()))
        } else {
            Err(Error::AlreadyTornDown)
        }
    }
}

/// `mozIBridgedSyncEngine` implementation.
impl TabsBridge {
    xpcom_method!(get_logger => GetLogger() -> *const mozIServicesLogSink);
    fn get_logger(&self) -> Result<RefPtr<mozIServicesLogSink>> {
        Err(NS_OK)?
    }

    xpcom_method!(set_logger => SetLogger(logger: *const mozIServicesLogSink));
    fn set_logger(&self, _logger: Option<&mozIServicesLogSink>) -> Result<()> {
        Ok(())
    }

    xpcom_method!(get_storage_version => GetStorageVersion() -> i32);
    fn get_storage_version(&self) -> Result<i32> {
        //SAM TODO: Need to investigate storage version
        Ok(1)
    }

    // It's possible that migration, or even merging, will result in records
    // too large for the server. We tolerate that (and hope that the addons do
    // too :)
    xpcom_method!(get_allow_skipped_record => GetAllowSkippedRecord() -> bool);
    fn get_allow_skipped_record(&self) -> Result<bool> {
        Ok(true)
    }

    xpcom_method!(
        get_last_sync => GetLastSync(
            callback: *const mozIBridgedSyncEngineCallback
        )
    );
    fn get_last_sync(&self, callback: &mozIBridgedSyncEngineCallback) -> Result<()> {
        let store = &*self.store()?;
        Ok(FerryTask::for_last_sync(store, callback)?.dispatch(&self.queue)?)
    }

    xpcom_method!(
        set_last_sync => SetLastSync(
            last_sync_millis: i64,
            callback: *const mozIBridgedSyncEngineCallback
        )
    );
    fn set_last_sync(
        &self,
        last_sync_millis: i64,
        callback: &mozIBridgedSyncEngineCallback,
    ) -> Result<()> {
        Ok(
            FerryTask::for_set_last_sync(&*self.store()?.lock(), last_sync_millis, callback)?
                .dispatch(&self.queue)?,
        )
    }

    xpcom_method!(
        get_sync_id => GetSyncId(
            callback: *const mozIBridgedSyncEngineCallback
        )
    );
    fn get_sync_id(&self, callback: &mozIBridgedSyncEngineCallback) -> Result<()> {
        Ok(FerryTask::for_sync_id(&*self.store()?.lock(), callback)?.dispatch(&self.queue)?)
    }

    xpcom_method!(
        reset_sync_id => ResetSyncId(
            callback: *const mozIBridgedSyncEngineCallback
        )
    );
    fn reset_sync_id(&self, callback: &mozIBridgedSyncEngineCallback) -> Result<()> {
        Ok(FerryTask::for_reset_sync_id(&*self.store()?, callback)?.dispatch(&self.queue)?)
    }

    xpcom_method!(
        ensure_current_sync_id => EnsureCurrentSyncId(
            new_sync_id: *const nsACString,
            callback: *const mozIBridgedSyncEngineCallback
        )
    );
    fn ensure_current_sync_id(
        &self,
        new_sync_id: &nsACString,
        callback: &mozIBridgedSyncEngineCallback,
    ) -> Result<()> {
        Ok(
            FerryTask::for_ensure_current_sync_id(&*self.store()?.lock(), new_sync_id, callback)?
                .dispatch(&self.queue)?,
        )
    }

    xpcom_method!(
        sync_started => SyncStarted(
            callback: *const mozIBridgedSyncEngineCallback
        )
    );
    fn sync_started(&self, callback: &mozIBridgedSyncEngineCallback) -> Result<()> {
        Ok(FerryTask::for_sync_started(&*self.store()?.lock(), callback)?.dispatch(&self.queue)?)
    }

    xpcom_method!(
        store_incoming => StoreIncoming(
            incoming_envelopes_json: *const ThinVec<::nsstring::nsCString>,
            callback: *const mozIBridgedSyncEngineCallback
        )
    );
    fn store_incoming(
        &self,
        incoming_envelopes_json: Option<&ThinVec<nsCString>>,
        callback: &mozIBridgedSyncEngineCallback,
    ) -> Result<()> {
        Ok(FerryTask::for_store_incoming(
            &*self.store()?.lock(),
            incoming_envelopes_json.map(|v| v.as_slice()).unwrap_or(&[]),
            callback,
        )?
        .dispatch(&self.queue)?)
    }

    xpcom_method!(apply => Apply(callback: *const mozIBridgedSyncEngineApplyCallback));
    fn apply(&self, callback: &mozIBridgedSyncEngineApplyCallback) -> Result<()> {
        Ok(ApplyTask::new(&*self.store()?.lock(), callback)?.dispatch(&self.queue)?)
    }

    xpcom_method!(
        set_uploaded => SetUploaded(
            server_modified_millis: i64,
            uploaded_ids: *const ThinVec<::nsstring::nsCString>,
            callback: *const mozIBridgedSyncEngineCallback
        )
    );
    fn set_uploaded(
        &self,
        server_modified_millis: i64,
        uploaded_ids: Option<&ThinVec<nsCString>>,
        callback: &mozIBridgedSyncEngineCallback,
    ) -> Result<()> {
        Ok(FerryTask::for_set_uploaded(
            &*self.store()?.lock(),
            server_modified_millis,
            uploaded_ids.map(|v| v.as_slice()).unwrap_or(&[]),
            callback,
        )?
        .dispatch(&self.queue)?)
    }

    xpcom_method!(
        sync_finished => SyncFinished(
            callback: *const mozIBridgedSyncEngineCallback
        )
    );
    fn sync_finished(&self, callback: &mozIBridgedSyncEngineCallback) -> Result<()> {
        Ok(
            FerryTask::for_sync_finished(&*self.store()?.lock(), callback)?
                .dispatch(&self.queue)?,
        )
    }

    xpcom_method!(
        reset => Reset(
            callback: *const mozIBridgedSyncEngineCallback
        )
    );
    fn reset(&self, callback: &mozIBridgedSyncEngineCallback) -> Result<()> {
        Ok(FerryTask::for_reset(&*self.store()?.lock(), callback)?.dispatch(&self.queue)?)
    }

    xpcom_method!(
        wipe => Wipe(
            callback: *const mozIBridgedSyncEngineCallback
        )
    );
    fn wipe(&self, callback: &mozIBridgedSyncEngineCallback) -> Result<()> {
        Ok(FerryTask::for_wipe(&*self.store()?.lock(), callback)?.dispatch(&self.queue)?)
    }
}
