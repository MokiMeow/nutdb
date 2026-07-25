//! MVCC garbage collection.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GcReport {
    pub watermark: u64,
    pub versions_reclaimed: usize,
}
