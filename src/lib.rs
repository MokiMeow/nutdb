//! nutdb — a distributed SQL database built from the durability layer up.
//!
//! The storage foundation is a checksummed write-ahead log plus a paged B-tree
//! with durable checkpoints. Later milestones add MVCC transactions, SQL, and Raft
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
pub mod catalog;
pub mod crc;
pub mod gc;
pub mod page;
pub mod pager;
pub mod store;
pub mod sql;
pub mod txn;
pub mod version;
pub mod wal;

pub use command::Command;
pub use btree::BTreeIndex;
pub use store::{Recovery, Store};
pub use txn::{MvccStore, Transaction, TxnError};
pub use sql::{SqlEngine, SqlResult, Value};
pub use wal::{ReplayResult, Wal};
