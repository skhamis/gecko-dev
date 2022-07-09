use std::path::Path;
use std::cell::RefCell;

pub struct TabsStore {
    // pub storage: &str,
}

impl TabsStore {
    pub fn new(db_path: impl AsRef<Path>) -> Self {
        Self {
            // storage: Mutex::new(TabsStorage::new(db_path)),
        }
    }
}