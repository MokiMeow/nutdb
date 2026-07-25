//! nutdb — a distributed SQL database built from the durability layer up.
//!
//! Milestone 0 is the foundation everything else stands on: a write-ahead log
//! with checksummed records, and a key-value store that survives a crash at any
//! point. Later milestones add MVCC transactions, a SQL layer, and Raft
//! replication — but none of those mean anything if a crash can lose or corrupt
//! committed data, so durability comes first.
//!
//! ```no_run
//! use nutdb::Store;
//!
//! let mut store = Store::open("data/nutdb.wal")?;
//! store.set("user:1", "ada")?;
//! assert_eq!(store.get("user:1"), Some("ada"));
//! # Ok::<(), std::io::Error>(())
//! ```

pub mod command;
pub mod btree;
pub mod crc;
pub mod page;
pub mod pager;
pub mod store;
pub mod wal;

pub use command::Command;
pub use btree::BTreeIndex;
pub use store::{Recovery, Store};
pub use wal::{ReplayResult, Wal};
