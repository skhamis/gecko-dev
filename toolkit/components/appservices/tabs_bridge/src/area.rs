/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */


 use std::{
    cell::{Ref, RefCell},
    convert::TryInto,
    ffi::OsString,
    mem,
    path::PathBuf,
    str,
    sync::Arc,
};

use golden_gate::{ApplyTask, FerryTask};
use moz_task::{self, DispatchOptions, TaskRunnable};
use nserror::{nsresult, NS_OK};
use nsstring::{nsACString, nsCString, nsString};
use thin_vec::ThinVec;
// use webext_storage::STORAGE_VERSION;
use xpcom::{
    interfaces::{
        mozIBridgedSyncEngineApplyCallback, mozIBridgedSyncEngineCallback,
        mozIExtensionStorageCallback, mozIServicesLogSink, nsIFile, nsISerialEventTarget,
    },
    RefPtr,
};

// use crate::error::{Error, Result};
// use crate::punt::{Punt, PuntTask, TeardownTask};
use crate::store::{LazyStore, LazyStoreConfig};

/// An XPCOM component class for the Rust extension storage API. This class
/// implements the interfaces needed for syncing and storage.
///
/// This class can be created on any thread, but must not be shared between
/// threads. In Rust terms, it's `Send`, but not `Sync`.
#[derive(xpcom)]
#[xpimplements(
    mozIExtensionStorageArea,
    mozIConfigurableExtensionStorageArea,
    mozISyncedExtensionStorageArea,
    mozIInterruptible,
    mozIBridgedSyncEngine
)]
#[refcnt = "nonatomic"]
pub struct InitStorageSyncArea {
    /// A background task queue, used to run all our storage operations on a
    /// thread pool. Using a serial event target here means that all operations
    /// will execute sequentially.
    queue: RefPtr<nsISerialEventTarget>,
    /// The store is lazily initialized on the task queue the first time it's
    /// used.
    store: RefCell<Option<Arc<LazyStore>>>,
}