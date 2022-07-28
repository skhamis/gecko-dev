/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use std::{
    fs::remove_file,
    mem,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
};

use crate::TabsEngine;
use golden_gate::{ApplyResults, BridgedEngine, Guid, IncomingEnvelope};
use interrupt_support::SqlInterruptHandle;
use once_cell::sync::OnceCell;
use tabs::TabsStore;

use crate::error::{Error, Result};

// Turns out we need this store as a layer of indirection because there are two "BridgedEngines"
// that are just slightly enough different that they had to be split for webext
// see bridge.rs in the tabs component in a-s and the BridgedEngine imported above for the differences
// though the impl<'a> sync15
// One of the earliest tasks should almost certaintly be to combine these two
pub struct TabsStoreBridge {
    inner: TabsStore,
}

impl TabsStoreBridge {
    pub fn get(&self) -> Result<TabsStore> {
        Ok(self.inner)
    }
}

impl BridgedEngine for TabsStoreBridge {
    type Error = Error;

    fn last_sync(&self) -> Result<i64> {
        Ok(Arc::new(self.get()?).bridged_engine().last_sync()?)
    }

    fn set_last_sync(&self, last_sync_millis: i64) -> Result<()> {
        Ok(Arc::new(self.get()?)
            .bridged_engine()
            .set_last_sync(last_sync_millis)?)
    }

    fn sync_id(&self) -> Result<Option<String>> {
        Ok(Arc::new(self.get()?).bridged_engine().sync_id()?)
    }

    fn reset_sync_id(&self) -> Result<String> {
        Ok(Arc::new(self.get()?).bridged_engine().reset_sync_id()?)
    }

    fn ensure_current_sync_id(&self, new_sync_id: &str) -> Result<String> {
        Ok(Arc::new(self.get()?)
            .bridged_engine()
            .ensure_current_sync_id(new_sync_id)?)
    }

    fn sync_started(&self) -> Result<()> {
        Ok(self.get()?.bridged_engine().sync_started()?)
    }

    fn store_incoming(&self, envelopes: &[IncomingEnvelope]) -> Result<()> {
        Ok(self.get()?.bridged_engine().store_incoming(envelopes)?)
    }

    fn apply(&self) -> Result<ApplyResults> {
        Ok(self.get()?.bridged_engine().apply()?)
    }

    fn set_uploaded(&self, server_modified_millis: i64, ids: &[Guid]) -> Result<()> {
        Ok(self
            .get()?
            .bridged_engine()
            .set_uploaded(server_modified_millis, ids)?)
    }

    fn sync_finished(&self) -> Result<()> {
        Ok(self.get()?.bridged_engine().sync_finished()?)
    }

    fn reset(&self) -> Result<()> {
        Ok(self.get()?.bridged_engine().reset()?)
    }

    fn wipe(&self) -> Result<()> {
        Ok(self.get()?.bridged_engine().wipe()?)
    }
}
