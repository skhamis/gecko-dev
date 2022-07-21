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

use crate::error::{Error, Result};

// Turns out we need this store as a layer of indirection because there are two "BridgedEngines"
// that are just slightly enough different that they had to be split for webext
// see bridge.rs in the tabs component in a-s and the BridgedEngine imported above for the differences
// though the impl<'a> sync15
// One of the earliest tasks should almost certaintly be to combine these two
pub struct TabsBridgeStore {
    inner: TabsEngine,
}

impl BridgedEngine for TabsBridgeStore {
    fn last_sync(&self) -> Result<i64> {
        Ok(0)
    }
}
