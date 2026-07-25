//! A durable key-value store: an in-memory map whose every mutation is written
//! to the log first.
//!
//! The ordering is the entire point:
//!
//! ```text
//! set(k, v)  →  append to WAL  →  fsync  →  update the map  →  return
//!                                    ↑
//!                     a crash before this point loses the write,
//!                     a crash after it does not — and either way
//!                     the store comes back self-consistent.
//! ```
//!
//! This is write-ahead logging, and it is the foundation every later milestone
//! (MVCC, SQL, Raft) is built on.

use std::collections::HashMap;
use std::io;
use std::path::Path;

use crate::command::Command;
use crate::wal::{ReplayResult, Wal};

pub struct Store {
    wal: Wal,
    data: HashMap<String, String>,
    recovery: Recovery,
}

/// What happened when the store was last opened.
#[derive(Debug, Default, Clone, Copy)]
pub struct Recovery {
    /// Records replayed from the log.
    pub records_replayed: usize,
    /// True if the log ended in a torn write that was skipped.
    pub truncated: bool,
    /// Byte length of the log's valid prefix.
    pub valid_bytes: u64,
}

impl Store {
    /// Open the store at `path`, replaying the log to rebuild state.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let ReplayResult {
            records,
            truncated,
            valid_bytes,
        } = Wal::replay(path.as_ref())?;

        let mut data = HashMap::new();
        let records_replayed = records.len();
        for record in records {
            match Command::decode(&record)? {
                Command::Set { key, value } => {
                    data.insert(key, value);
                }
                Command::Delete { key } => {
                    data.remove(&key);
                }
            }
        }

        Ok(Self {
            wal: Wal::open(path)?,
            data,
            recovery: Recovery {
                records_replayed,
                truncated,
                valid_bytes,
            },
        })
    }

    /// Durably store `key` = `value`.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) -> io::Result<()> {
        let key = key.into();
        let value = value.into();
        self.apply(Command::Set { key, value })
    }

    /// Durably remove `key`.
    pub fn delete(&mut self, key: impl Into<String>) -> io::Result<()> {
        self.apply(Command::Delete { key: key.into() })
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.data.get(key).map(String::as_str)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.data.contains_key(key)
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Keys, sorted — handy for deterministic output and tests.
    pub fn keys_sorted(&self) -> Vec<&str> {
        let mut keys: Vec<&str> = self.data.keys().map(String::as_str).collect();
        keys.sort_unstable();
        keys
    }

    /// Details of the most recent recovery.
    pub fn recovery(&self) -> Recovery {
        self.recovery
    }

    pub fn wal_size(&self) -> io::Result<u64> {
        self.wal.size()
    }

    /// Log first, sync, *then* mutate memory. Never the other way round.
    fn apply(&mut self, command: Command) -> io::Result<()> {
        self.wal.append(&command.encode())?;
        self.wal.sync()?;
        match command {
            Command::Set { key, value } => {
                self.data.insert(key, value);
            }
            Command::Delete { key } => {
                self.data.remove(&key);
            }
        }
        Ok(())
    }
}
