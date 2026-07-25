//! A durable B-tree-backed key-value store.
//!
//! Every acknowledged mutation follows `WAL append -> fsync -> B-tree mutate`.
//! A checkpoint writes and syncs a complete checksummed page snapshot before it
//! records the checkpoint and truncates the now-redundant WAL. A crash at any
//! intermediate point therefore leaves either the old snapshot plus the full
//! log, or the new snapshot plus an idempotently replayable/logically empty
//! log.

use std::io;
use std::path::{Path, PathBuf};

use crate::btree::BTreeIndex;
use crate::command::Command;
use crate::wal::{ReplayResult, Wal};

pub struct Store {
    wal: Wal,
    page_path: PathBuf,
    data: BTreeIndex,
    recovery: Recovery,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Recovery {
    pub records_replayed: usize,
    pub truncated: bool,
    pub valid_bytes: u64,
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        let page_path = page_path(path);
        let ReplayResult {
            records,
            truncated,
            valid_bytes,
        } = Wal::replay(path)?;

        let mut data = BTreeIndex::load(&page_path)?;
        let records_replayed = records.len();
        for record in records {
            apply_memory(&mut data, Command::decode(&record)?);
        }

        if truncated {
            Wal::truncate_path(path, valid_bytes)?;
        }

        Ok(Self {
            wal: Wal::open(path)?,
            page_path,
            data,
            recovery: Recovery {
                records_replayed,
                truncated,
                valid_bytes,
            },
        })
    }

    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) -> io::Result<()> {
        self.apply(Command::Set {
            key: key.into(),
            value: value.into(),
        })
    }

    pub fn delete(&mut self, key: impl Into<String>) -> io::Result<()> {
        self.apply(Command::Delete { key: key.into() })
    }

    /// Commit several mutations with one durability flush.
    pub fn apply_batch(&mut self, commands: Vec<Command>) -> io::Result<()> {
        if commands.iter().any(|command| matches!(command, Command::Checkpoint)) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "store: checkpoint is not a user mutation",
            ));
        }
        for command in &commands {
            self.wal.append(&command.encode())?;
        }
        self.wal.sync()?;
        for command in commands {
            apply_memory(&mut self.data, command);
        }
        Ok(())
    }

    pub fn set_batch<I, K, V>(&mut self, entries: I) -> io::Result<()>
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let commands = entries
            .into_iter()
            .map(|(key, value)| Command::Set {
                key: key.into(),
                value: value.into(),
            })
            .collect();
        self.apply_batch(commands)
    }

    /// Persist the current B-tree and remove the WAL prefix it supersedes.
    pub fn checkpoint(&mut self) -> io::Result<()> {
        self.data
            .save(&self.page_path)
            .map_err(|error| stage_error("write page snapshot", error))?;
        self.wal
            .append(&Command::Checkpoint.encode())
            .map_err(|error| stage_error("append checkpoint record", error))?;
        self.wal
            .sync()
            .map_err(|error| stage_error("sync checkpoint record", error))?;
        self.wal
            .reset()
            .map_err(|error| stage_error("truncate checkpointed WAL", error))?;
        Ok(())
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.data.get(key)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.data.get(key).is_some()
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn keys_sorted(&self) -> Vec<&str> {
        self.data.keys()
    }

    pub fn range(&self, start: &str, end: &str) -> Vec<(String, String)> {
        self.data.range(start, end)
    }

    pub fn recovery(&self) -> Recovery {
        self.recovery
    }

    pub fn wal_size(&self) -> io::Result<u64> {
        self.wal.size()
    }

    fn apply(&mut self, command: Command) -> io::Result<()> {
        self.wal.append(&command.encode())?;
        self.wal.sync()?;
        apply_memory(&mut self.data, command);
        Ok(())
    }
}

fn apply_memory(data: &mut BTreeIndex, command: Command) {
    match command {
        Command::Set { key, value } => data.insert(key, value),
        Command::Delete { key } => {
            data.delete(&key);
        }
        Command::Checkpoint => {}
    }
}

fn page_path(wal_path: &Path) -> PathBuf {
    let mut path = wal_path.as_os_str().to_os_string();
    path.push(".pages");
    PathBuf::from(path)
}

fn stage_error(stage: &str, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("store: failed to {stage}: {error}"))
}
