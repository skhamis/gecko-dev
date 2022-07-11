/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use std::cell::RefCell;
use std::sync::Arc;

use crate::error::{Result, TabsError};
use crate::{TabsEngine, TabsStore};
//use rusqlite::Transaction;
use sync15::{EngineSyncAssociation, ServerTimestamp, SyncEngine};
use sync15_traits::{
    self, telemetry::Engine, ApplyResults, IncomingChangeset, IncomingEnvelope, OutgoingEnvelope,
    Payload,
};
use sync_guid::Guid as SyncGuid;

/// A bridged engine implements all the methods needed to make the
/// `storage.sync` store work with Desktop's Sync implementation.
/// Conceptually, it's similar to `sync15_traits::Store`, which we
/// should eventually rename and unify with this trait (#2841).
pub struct BridgedEngine {
    store: Arc<TabsStore>,
    incoming_payload: RefCell<Vec<Payload>>,
}

impl<'a> BridgedEngine {
    /// Creates a bridged engine for syncing.
    pub fn new(store: Arc<TabsStore>) -> Self {
        BridgedEngine {
            store,
            incoming_payload: RefCell::default(),
        }
    }

    // fn do_reset(&self, tx: &Transaction<'_>) -> Result<()> {
    //     let engine = &TabsEngine::new(Arc::clone(&self.store));
    //     let _ = engine.wipe();
    //     Ok(())
    // }
}

impl<'a> sync15_traits::BridgedEngine for BridgedEngine {
    type Error = TabsError;

    fn last_sync(&self) -> Result<i64> {
        let engine = &TabsEngine::new(Arc::clone(&self.store));
        Ok(engine.last_sync.get().unwrap_or_default().as_millis())
    }

    fn set_last_sync(&self, last_sync_millis: i64) -> Result<()> {
        //TODO: Should we instead make an API in the engine for setting this?
        let engine = &TabsEngine::new(Arc::clone(&self.store));
        let _ = &engine
            .last_sync
            .set(Some(ServerTimestamp::from_millis(last_sync_millis)));
        Ok(())
    }

    fn sync_id(&self) -> Result<Option<String>> {
        Ok(Some(
            TabsEngine::new(Arc::clone(&self.store))
                .local_id
                .borrow()
                .clone(),
        ))
    }

    fn reset_sync_id(&self) -> Result<String> {
        //TODO: tabs sets the local_id in prepare_for_sync and sets it to the client id
        //let engine = &TabsEngine::new(Arc::clone(&self.store));
        let new_id = SyncGuid::random().to_string();
        Ok(new_id)
    }

    fn ensure_current_sync_id(&self, sync_id: &str) -> Result<String> {
        let engine = &TabsEngine::new(Arc::clone(&self.store));
        let current: Option<String> = Some(engine.local_id.borrow().clone());
        Ok(match current {
            Some(current) if current == sync_id => current,
            _ => {
                //TODO: Probably pretty hacky to just force the tabs engine to use whatever is on the server
                // need to figure out the proper way to either reset or modify the table
                let result = sync_id.to_string();
                engine.local_id.replace(result.clone());
                result
            }
        })
    }

    fn sync_started(&self) -> Result<()> {
        Ok(())
    }

    fn store_incoming(&self, incoming_envelopes: &[IncomingEnvelope]) -> Result<()> {
        let mut incoming_payloads = Vec::with_capacity(incoming_envelopes.len());
        for envelope in incoming_envelopes {
            incoming_payloads.push(envelope.payload()?);
        }
        // Store the incoming payload in memory so we can use it in apply
        self.incoming_payload.replace(incoming_payloads);
        Ok(())
    }

    fn apply(&self) -> Result<ApplyResults> {
        let engine = &TabsEngine::new(Arc::clone(&self.store));
        let mut incoming = IncomingChangeset::new(engine.collection_name(), ServerTimestamp(0));
        let incoming_payload = self.incoming_payload.borrow().clone().into_iter();

        for payload in incoming_payload {
            // TODO: Need a better way to determine timestamp
            incoming.changes.push((payload, ServerTimestamp(0)));
        }

        let outgoing_changeset = engine.apply_incoming(vec![incoming], &mut Engine::new("tabs"))?;

        let outgoing = outgoing_changeset
            .changes
            .into_iter()
            .map(OutgoingEnvelope::from)
            .collect::<Vec<_>>();

        Ok(ApplyResults {
            envelopes: outgoing,
            num_reconciled: Some(0),
        })
    }

    fn set_uploaded(&self, _server_modified_millis: i64, _ids: &[SyncGuid]) -> Result<()> {
        //TODO: Finish this
        Ok(())
    }

    fn sync_finished(&self) -> Result<()> {
        let _ = &self.incoming_payload.replace(Vec::default());
        Ok(())
    }

    fn reset(&self) -> Result<()> {
        let engine = &TabsEngine::new(Arc::clone(&self.store));
        let _ = engine.reset(&EngineSyncAssociation::Disconnected);
        Ok(())
    }

    fn wipe(&self) -> Result<()> {
        let engine = &TabsEngine::new(Arc::clone(&self.store));
        let _ = engine.wipe();
        Ok(())
    }
}

// TODO: Copied from webext -- Update them for tabs purposes
// #[cfg(test)]
// mod tests {
//     use super::*;
//     use crate::storage::TabsStorage;
//     use sync15_traits::bridged_engine::BridgedEngine;

//     fn query_count(conn: &TabsStorage, table: &str) -> u32 {
//         conn.query_row_and_then(&format!("SELECT COUNT(*) FROM {};", table), [], |row| {
//             row.get::<_, u32>(0)
//         })
//         .expect("should work")
//     }

//     // Sets up mock data for the tests here.
//     fn setup_mock_data(engine: &super::BridgedEngine<'_>) -> Result<()> {
//         engine.db.lock().unwrap().execute(
//             "INSERT INTO storage_sync_data (ext_id, data, sync_change_counter)
//                   VALUES ('ext-a', 'invalid-json', 2)",
//             [],
//         )?;
//         engine.db.lock().unwrap().execute(
//             "INSERT INTO storage_sync_mirror (guid, ext_id, data)
//                   VALUES ('guid', 'ext-a', '3')",
//             [],
//         )?;
//         engine.set_last_sync(1)?;

//         // and assert we wrote what we think we did.
//         assert_eq!(
//             query_count(&engine.db.lock().unwrap(), "storage_sync_data"),
//             1
//         );
//         assert_eq!(
//             query_count(&engine.db.lock().unwrap(), "storage_sync_mirror"),
//             1
//         );
//         assert_eq!(query_count(&engine.db.lock().unwrap(), "meta"), 1);
//         Ok(())
//     }

//     // Assuming a DB setup with setup_mock_data, assert it was correctly reset.
//     fn assert_reset(engine: &super::BridgedEngine<'_>) -> Result<()> {
//         // A reset never wipes data...
//         assert_eq!(
//             query_count(&engine.db.lock().unwrap(), "storage_sync_data"),
//             1
//         );

//         // But did reset the change counter.
//         let cc = engine.db.lock().unwrap().query_row_and_then(
//             "SELECT sync_change_counter FROM storage_sync_data WHERE ext_id = 'ext-a';",
//             [],
//             |row| row.get::<_, u32>(0),
//         )?;
//         assert_eq!(cc, 1);
//         // But did wipe the mirror...
//         assert_eq!(
//             query_count(&engine.db.lock().unwrap(), "storage_sync_mirror"),
//             0
//         );
//         // And the last_sync should have been wiped.
//         assert!(get_meta::<i64>(&engine.db.lock().unwrap(), LAST_SYNC_META_KEY)?.is_none());
//         Ok(())
//     }

//     // Assuming a DB setup with setup_mock_data, assert it has not been reset.
//     fn assert_not_reset(engine: &super::BridgedEngine<'_>) -> Result<()> {
//         assert_eq!(
//             query_count(&engine.db.lock().unwrap(), "storage_sync_data"),
//             1
//         );
//         let cc = engine.db.lock().unwrap().query_row_and_then(
//             "SELECT sync_change_counter FROM storage_sync_data WHERE ext_id = 'ext-a';",
//             [],
//             |row| row.get::<_, u32>(0),
//         )?;
//         assert_eq!(cc, 2);
//         assert_eq!(
//             query_count(&engine.db.lock().unwrap(), "storage_sync_mirror"),
//             1
//         );
//         // And the last_sync should remain.
//         assert!(get_meta::<i64>(&engine.db.lock().unwrap(), LAST_SYNC_META_KEY)?.is_some());
//         Ok(())
//     }

//     #[test]
//     fn test_wipe() -> Result<()> {
//         let db = Mutex::new(TabsStorage::new_with_mem_path("test"));
//         let engine = super::BridgedEngine::new(&db);

//         setup_mock_data(&engine)?;

//         engine.wipe()?;
//         assert_eq!(
//             query_count(&engine.db.lock().unwrap(), "storage_sync_data"),
//             0
//         );
//         assert_eq!(
//             query_count(&engine.db.lock().unwrap(), "storage_sync_mirror"),
//             0
//         );
//         assert_eq!(query_count(&engine.db.lock().unwrap(), "meta"), 0);
//         Ok(())
//     }

//     #[test]
//     fn test_reset() -> Result<()> {
//         let db = Mutex::new(TabsStorage::new_with_mem_path("test"));
//         let engine = super::BridgedEngine::new(&db);

//         setup_mock_data(&engine)?;
//         put_meta(
//             &engine.db.lock().unwrap(),
//             SYNC_ID_META_KEY,
//             &"sync-id".to_string(),
//         )?;

//         engine.reset()?;
//         assert_reset(&engine)?;
//         // Only an explicit reset kills the sync-id, so check that here.
//         assert_eq!(
//             get_meta::<String>(&engine.db.lock().unwrap(), SYNC_ID_META_KEY)?,
//             None
//         );

//         Ok(())
//     }

//     #[test]
//     fn test_ensure_missing_sync_id() -> Result<()> {
//         let db = Mutex::new(TabsStorage::new_with_mem_path("test"));
//         let engine = super::BridgedEngine::new(&db);

//         setup_mock_data(&engine)?;

//         assert_eq!(engine.sync_id()?, None);
//         // We don't have a sync ID - so setting one should reset.
//         engine.ensure_current_sync_id("new-id")?;
//         // should have cause a reset.
//         assert_reset(&engine)?;
//         Ok(())
//     }

//     #[test]
//     fn test_ensure_new_sync_id() -> Result<()> {
//         let db = Mutex::new(TabsStorage::new_with_mem_path("test"));
//         let engine = super::BridgedEngine::new(&db);

//         setup_mock_data(&engine)?;

//         put_meta(
//             &engine.db.lock().unwrap(),
//             SYNC_ID_META_KEY,
//             &"old-id".to_string(),
//         )?;
//         assert_not_reset(&engine)?;
//         assert_eq!(engine.sync_id()?, Some("old-id".to_string()));

//         engine.ensure_current_sync_id("new-id")?;
//         // should have cause a reset.
//         assert_reset(&engine)?;
//         // should have the new id.
//         assert_eq!(engine.sync_id()?, Some("new-id".to_string()));
//         Ok(())
//     }

//     #[test]
//     fn test_ensure_same_sync_id() -> Result<()> {
//         let db = Mutex::new(TabsStorage::new_with_mem_path("test"));
//         let engine = super::BridgedEngine::new(&db);

//         setup_mock_data(&engine)?;
//         assert_not_reset(&engine)?;

//         put_meta(
//             &engine.db.lock().unwrap(),
//             SYNC_ID_META_KEY,
//             &"sync-id".to_string(),
//         )?;

//         engine.ensure_current_sync_id("sync-id")?;
//         // should not have reset.
//         assert_not_reset(&engine)?;
//         Ok(())
//     }

//     #[test]
//     fn test_reset_sync_id() -> Result<()> {
//         let db = Mutex::new(TabsStorage::new_with_mem_path("test"));
//         let engine = super::BridgedEngine::new(&db);

//         setup_mock_data(&engine)?;
//         put_meta(
//             &engine.db.lock().unwrap(),
//             SYNC_ID_META_KEY,
//             &"sync-id".to_string(),
//         )?;

//         assert_eq!(engine.sync_id()?, Some("sync-id".to_string()));
//         let new_id = engine.reset_sync_id()?;
//         // should have cause a reset.
//         assert_reset(&engine)?;
//         assert_eq!(engine.sync_id()?, Some(new_id));
//         Ok(())
//     }
//}
