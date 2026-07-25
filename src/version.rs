//! Versioned values and snapshot visibility.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub value: String,
    pub created_at: u64,
    pub deleted_at: Option<u64>,
}

impl Version {
    pub fn visible_at(&self, snapshot: u64) -> bool {
        self.created_at <= snapshot
            && self
                .deleted_at
                .map(|deleted| deleted > snapshot)
                .unwrap_or(true)
    }

    pub fn last_modified(&self) -> u64 {
        self.deleted_at.unwrap_or(self.created_at).max(self.created_at)
    }
}
