//! Snapshot-isolated transactions with durable commit records.
//!
//! Transaction ids identify attempts and are persisted at `begin`, so a crash
//! cannot cause reuse. Commit timestamps define visibility and are assigned
//! only while holding the store lock. Keeping these two counters separate is
//! essential: transaction 1 may commit after transaction 2, and snapshots must
//! follow commit order rather than allocation order.
//!
//! WAL records are checksummed by [`crate::wal::Wal`]. Their payload layout is:
//!
//! ```text
//! Begin:  [0x10][txn:u64][snapshot:u64]
//! Set:    [0x11][txn:u64][klen:u32][key][vlen:u32][value]
//! Delete: [0x12][txn:u64][klen:u32][key]
//! Commit: [0x13][txn:u64][commit_ts:u64]
//! Abort:  [0x14][txn:u64]
//! ```
//!
//! Writes precede the commit record. Recovery buffers them and applies the
//! batch only if it later sees an intact commit record.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::gc::GcReport;
use crate::version::Version;
use crate::wal::{ReplayResult, Wal};

const TAG_BEGIN: u8 = 0x10;
const TAG_SET: u8 = 0x11;
const TAG_DELETE: u8 = 0x12;
const TAG_COMMIT: u8 = 0x13;
const TAG_ABORT: u8 = 0x14;

#[derive(Clone)]
pub struct MvccStore {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    wal: Wal,
    versions: BTreeMap<String, Vec<Version>>,
    active: BTreeMap<u64, u64>,
    next_txn: u64,
    commit_ts: u64,
}

pub struct Transaction {
    store: MvccStore,
    id: u64,
    snapshot: u64,
    writes: BTreeMap<String, Option<String>>,
    finished: bool,
}

#[derive(Debug)]
pub enum TxnError {
    Io(io::Error),
    Conflict { key: String },
    Finished,
}

impl fmt::Display for TxnError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Conflict { key } => write!(formatter, "write conflict on key '{key}'"),
            Self::Finished => write!(formatter, "transaction is already finished"),
        }
    }
}

impl std::error::Error for TxnError {}

impl From<io::Error> for TxnError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl MvccStore {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        let ReplayResult {
            records,
            truncated,
            valid_bytes,
        } = Wal::replay(path)?;

        let mut versions = BTreeMap::new();
        let mut pending: HashMap<u64, BTreeMap<String, Option<String>>> = HashMap::new();
        let mut begun = HashMap::new();
        let mut max_txn = 0;
        let mut commit_ts = 0;

        for payload in records {
            match TxnRecord::decode(&payload)? {
                TxnRecord::Begin { txn, snapshot } => {
                    max_txn = max_txn.max(txn);
                    begun.insert(txn, snapshot);
                    pending.entry(txn).or_default();
                }
                TxnRecord::Set { txn, key, value } => {
                    ensure_begun(&begun, txn)?;
                    pending.entry(txn).or_default().insert(key, Some(value));
                }
                TxnRecord::Delete { txn, key } => {
                    ensure_begun(&begun, txn)?;
                    pending.entry(txn).or_default().insert(key, None);
                }
                TxnRecord::Commit {
                    txn,
                    commit_ts: timestamp,
                } => {
                    ensure_begun(&begun, txn)?;
                    if timestamp <= commit_ts {
                        return Err(invalid("mvcc: commit timestamps are not increasing"));
                    }
                    let writes = pending.remove(&txn).unwrap_or_default();
                    apply_versions(&mut versions, writes, timestamp);
                    commit_ts = timestamp;
                    begun.remove(&txn);
                }
                TxnRecord::Abort { txn } => {
                    pending.remove(&txn);
                    begun.remove(&txn);
                }
            }
        }

        if truncated {
            Wal::truncate_path(path, valid_bytes)?;
        }
        Ok(Self {
            inner: Arc::new(Mutex::new(Inner {
                wal: Wal::open(path)?,
                versions,
                active: BTreeMap::new(),
                next_txn: max_txn
                    .checked_add(1)
                    .ok_or_else(|| invalid("mvcc: transaction id exhausted"))?,
                commit_ts,
            })),
        })
    }

    pub fn begin(&self) -> Result<Transaction, TxnError> {
        let mut inner = self.lock()?;
        let id = inner.next_txn;
        inner.next_txn = inner
            .next_txn
            .checked_add(1)
            .ok_or_else(|| invalid("mvcc: transaction id exhausted"))?;
        let snapshot = inner.commit_ts;
        inner
            .wal
            .append(&TxnRecord::Begin { txn: id, snapshot }.encode()?)?;
        inner.wal.sync()?;
        inner.active.insert(id, snapshot);
        drop(inner);
        Ok(Transaction {
            store: self.clone(),
            id,
            snapshot,
            writes: BTreeMap::new(),
            finished: false,
        })
    }

    pub fn get_at(&self, key: &str, snapshot: u64) -> io::Result<Option<String>> {
        let inner = self.lock().map_err(txn_to_io)?;
        Ok(visible_value(&inner.versions, key, snapshot))
    }

    pub fn current_timestamp(&self) -> io::Result<u64> {
        Ok(self.lock().map_err(txn_to_io)?.commit_ts)
    }

    pub fn version_count(&self, key: &str) -> io::Result<usize> {
        Ok(self
            .lock()
            .map_err(txn_to_io)?
            .versions
            .get(key)
            .map(Vec::len)
            .unwrap_or(0))
    }

    pub fn gc(&self) -> io::Result<GcReport> {
        let mut inner = self.lock().map_err(txn_to_io)?;
        let watermark = inner
            .active
            .values()
            .copied()
            .min()
            .unwrap_or(inner.commit_ts.saturating_add(1));
        let mut reclaimed = 0;
        inner.versions.retain(|_, chain| {
            let before = chain.len();
            chain.retain(|version| {
                !version
                    .deleted_at
                    .map(|deleted| deleted < watermark)
                    .unwrap_or(false)
            });
            reclaimed += before - chain.len();
            !chain.is_empty()
        });
        Ok(GcReport {
            watermark,
            versions_reclaimed: reclaimed,
        })
    }

    fn lock(&self) -> Result<MutexGuard<'_, Inner>, TxnError> {
        self.inner
            .lock()
            .map_err(|_| {
                TxnError::Io(io::Error::new(
                    io::ErrorKind::Other,
                    "mvcc: lock poisoned",
                ))
            })
    }
}

impl Transaction {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn snapshot(&self) -> u64 {
        self.snapshot
    }

    pub fn get(&self, key: &str) -> Result<Option<String>, TxnError> {
        self.ensure_open()?;
        if let Some(value) = self.writes.get(key) {
            return Ok(value.clone());
        }
        self.store.get_at(key, self.snapshot).map_err(TxnError::Io)
    }

    pub fn scan_prefix(&self, prefix: &str) -> Result<Vec<(String, String)>, TxnError> {
        self.ensure_open()?;
        let inner = self.store.lock()?;
        let mut rows = BTreeMap::new();
        for key in inner.versions.keys().filter(|key| key.starts_with(prefix)) {
            if let Some(value) = visible_value(&inner.versions, key, self.snapshot) {
                rows.insert(key.clone(), value);
            }
        }
        drop(inner);
        for (key, value) in self.writes.range(prefix.to_owned()..) {
            if !key.starts_with(prefix) {
                break;
            }
            match value {
                Some(value) => {
                    rows.insert(key.clone(), value.clone());
                }
                None => {
                    rows.remove(key);
                }
            }
        }
        Ok(rows.into_iter().collect())
    }

    pub fn set(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<(), TxnError> {
        self.ensure_open()?;
        let key = key.into();
        let value = value.into();
        let mut inner = self.store.lock()?;
        inner.wal.append(
            &TxnRecord::Set {
                txn: self.id,
                key: key.clone(),
                value: value.clone(),
            }
            .encode()?,
        )?;
        self.writes.insert(key, Some(value));
        Ok(())
    }

    pub fn delete(&mut self, key: impl Into<String>) -> Result<(), TxnError> {
        self.ensure_open()?;
        let key = key.into();
        let mut inner = self.store.lock()?;
        inner.wal.append(
            &TxnRecord::Delete {
                txn: self.id,
                key: key.clone(),
            }
            .encode()?,
        )?;
        self.writes.insert(key, None);
        Ok(())
    }

    pub fn commit(mut self) -> Result<u64, TxnError> {
        self.ensure_open()?;
        let mut inner = self.store.lock()?;
        for key in self.writes.keys() {
            if latest_change(&inner.versions, key) > self.snapshot {
                inner
                    .wal
                    .append(&TxnRecord::Abort { txn: self.id }.encode()?)?;
                inner.wal.sync()?;
                inner.active.remove(&self.id);
                self.finished = true;
                return Err(TxnError::Conflict { key: key.clone() });
            }
        }

        let timestamp = inner
            .commit_ts
            .checked_add(1)
            .ok_or_else(|| invalid("mvcc: commit timestamp exhausted"))?;
        inner.wal.append(
            &TxnRecord::Commit {
                txn: self.id,
                commit_ts: timestamp,
            }
            .encode()?,
        )?;
        inner.wal.sync()?;
        apply_versions(&mut inner.versions, std::mem::take(&mut self.writes), timestamp);
        inner.commit_ts = timestamp;
        inner.active.remove(&self.id);
        self.finished = true;
        Ok(timestamp)
    }

    pub fn abort(mut self) -> Result<(), TxnError> {
        self.ensure_open()?;
        let mut inner = self.store.lock()?;
        inner
            .wal
            .append(&TxnRecord::Abort { txn: self.id }.encode()?)?;
        inner.wal.sync()?;
        inner.active.remove(&self.id);
        self.finished = true;
        Ok(())
    }

    fn ensure_open(&self) -> Result<(), TxnError> {
        if self.finished {
            Err(TxnError::Finished)
        } else {
            Ok(())
        }
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        if !self.finished {
            if let Ok(mut inner) = self.store.inner.lock() {
                inner.active.remove(&self.id);
            }
        }
    }
}

fn visible_value(
    versions: &BTreeMap<String, Vec<Version>>,
    key: &str,
    snapshot: u64,
) -> Option<String> {
    versions
        .get(key)?
        .iter()
        .rev()
        .find(|version| version.visible_at(snapshot))
        .map(|version| version.value.clone())
}

fn latest_change(versions: &BTreeMap<String, Vec<Version>>, key: &str) -> u64 {
    versions
        .get(key)
        .and_then(|chain| chain.last())
        .map(Version::last_modified)
        .unwrap_or(0)
}

fn apply_versions(
    versions: &mut BTreeMap<String, Vec<Version>>,
    writes: BTreeMap<String, Option<String>>,
    timestamp: u64,
) {
    for (key, value) in writes {
        let chain = versions.entry(key).or_default();
        if let Some(current) = chain.last_mut() {
            if current.deleted_at.is_none() {
                current.deleted_at = Some(timestamp);
            }
        }
        if let Some(value) = value {
            chain.push(Version {
                value,
                created_at: timestamp,
                deleted_at: None,
            });
        }
    }
}

fn ensure_begun(begun: &HashMap<u64, u64>, txn: u64) -> io::Result<()> {
    if begun.contains_key(&txn) {
        Ok(())
    } else {
        Err(invalid("mvcc: record references unknown transaction"))
    }
}

#[derive(Debug)]
enum TxnRecord {
    Begin { txn: u64, snapshot: u64 },
    Set { txn: u64, key: String, value: String },
    Delete { txn: u64, key: String },
    Commit { txn: u64, commit_ts: u64 },
    Abort { txn: u64 },
}

impl TxnRecord {
    fn encode(&self) -> io::Result<Vec<u8>> {
        let mut out = Vec::new();
        match self {
            Self::Begin { txn, snapshot } => {
                out.push(TAG_BEGIN);
                put_u64(&mut out, *txn);
                put_u64(&mut out, *snapshot);
            }
            Self::Set { txn, key, value } => {
                out.push(TAG_SET);
                put_u64(&mut out, *txn);
                put_string(&mut out, key)?;
                put_string(&mut out, value)?;
            }
            Self::Delete { txn, key } => {
                out.push(TAG_DELETE);
                put_u64(&mut out, *txn);
                put_string(&mut out, key)?;
            }
            Self::Commit { txn, commit_ts } => {
                out.push(TAG_COMMIT);
                put_u64(&mut out, *txn);
                put_u64(&mut out, *commit_ts);
            }
            Self::Abort { txn } => {
                out.push(TAG_ABORT);
                put_u64(&mut out, *txn);
            }
        }
        Ok(out)
    }

    fn decode(bytes: &[u8]) -> io::Result<Self> {
        let mut cursor = Cursor { bytes, offset: 0 };
        let tag = cursor.u8()?;
        let txn = cursor.u64()?;
        let record = match tag {
            TAG_BEGIN => Self::Begin {
                txn,
                snapshot: cursor.u64()?,
            },
            TAG_SET => Self::Set {
                txn,
                key: cursor.string()?,
                value: cursor.string()?,
            },
            TAG_DELETE => Self::Delete {
                txn,
                key: cursor.string()?,
            },
            TAG_COMMIT => Self::Commit {
                txn,
                commit_ts: cursor.u64()?,
            },
            TAG_ABORT => Self::Abort { txn },
            _ => return Err(invalid("mvcc: unknown record tag")),
        };
        if cursor.offset != bytes.len() {
            return Err(invalid("mvcc: trailing record bytes"));
        }
        Ok(record)
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl Cursor<'_> {
    fn u8(&mut self) -> io::Result<u8> {
        let value = *self
            .bytes
            .get(self.offset)
            .ok_or_else(|| invalid("mvcc: short record"))?;
        self.offset += 1;
        Ok(value)
    }

    fn u64(&mut self) -> io::Result<u64> {
        let end = self
            .offset
            .checked_add(8)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| invalid("mvcc: short u64"))?;
        let value = u64::from_le_bytes(
            self.bytes[self.offset..end]
                .try_into()
                .expect("fixed slice"),
        );
        self.offset = end;
        Ok(value)
    }

    fn string(&mut self) -> io::Result<String> {
        let end = self
            .offset
            .checked_add(4)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| invalid("mvcc: short string length"))?;
        let len = u32::from_le_bytes(
            self.bytes[self.offset..end]
                .try_into()
                .expect("fixed slice"),
        ) as usize;
        self.offset = end;
        let end = self
            .offset
            .checked_add(len)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| invalid("mvcc: string exceeds record"))?;
        let value = std::str::from_utf8(&self.bytes[self.offset..end])
            .map_err(|_| invalid("mvcc: string is not UTF-8"))?
            .to_owned();
        self.offset = end;
        Ok(value)
    }
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_string(out: &mut Vec<u8>, value: &str) -> io::Result<()> {
    let len = u32::try_from(value.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "mvcc: string too large"))?;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

fn txn_to_io(error: TxnError) -> io::Error {
    match error {
        TxnError::Io(error) => error,
        other => io::Error::new(io::ErrorKind::Other, other.to_string()),
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
