//! The write-ahead log: the reason a crash does not lose committed data.
//!
//! Every mutation is appended here and `fsync`ed **before** it is applied to
//! in-memory state. After a crash the log is replayed to rebuild that state.
//!
//! Record layout (all integers little-endian):
//!
//! ```text
//! ┌──────────┬──────────┬─────────────────┐
//! │ len: u32 │ crc: u32 │ payload: len B  │
//! └──────────┴──────────┴─────────────────┘
//! ```
//!
//! The CRC covers the payload. Replay stops at the first record that is
//! incomplete or fails its checksum — that is a *torn write* from a crash
//! mid-append, and everything after it is untrustworthy by definition.

use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};

use crate::crc::crc32;

const HEADER_LEN: usize = 8; // len (4) + crc (4)

/// An append-only, checksummed log file.
pub struct Wal {
    file: File,
    path: PathBuf,
}

impl Wal {
    /// Open (or create) the log at `path` for appending.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .append(true)
            .open(&path)?;
        Ok(Self { file, path })
    }

    /// Append one record and flush it to the operating system.
    ///
    /// Note this does not `fsync` — call [`Wal::sync`] when the caller needs
    /// the durability guarantee. Separating the two lets a batch of writes
    /// share a single (expensive) flush.
    pub fn append(&mut self, payload: &[u8]) -> io::Result<()> {
        let len = u32::try_from(payload.len()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "wal: record exceeds 4 GiB")
        })?;

        // One buffer, one write call: a single write is far less likely to be
        // torn than three, though the checksum is what actually makes recovery
        // correct if it happens anyway.
        let mut record = Vec::with_capacity(HEADER_LEN + payload.len());
        record.extend_from_slice(&len.to_le_bytes());
        record.extend_from_slice(&crc32(payload).to_le_bytes());
        record.extend_from_slice(payload);

        self.file.write_all(&record)
    }

    /// Force everything written so far to durable storage.
    pub fn sync(&mut self) -> io::Result<()> {
        self.file.sync_all()
    }

    /// Remove every byte from the log after a durable checkpoint.
    pub fn reset(&mut self) -> io::Result<()> {
        // Windows grants append handles FILE_APPEND_DATA rather than the
        // FILE_WRITE_DATA right required by SetEndOfFile, so truncate through
        // a short-lived ordinary write handle.
        let file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&self.path)?;
        file.sync_all()
    }

    /// Discard a torn or corrupt tail before accepting new records.
    ///
    /// Without this repair, appending after a bad record would place valid
    /// data behind a prefix replay must always stop at.
    pub fn truncate_path(path: impl AsRef<Path>, valid_bytes: u64) -> io::Result<()> {
        let file = OpenOptions::new().write(true).open(path)?;
        file.set_len(valid_bytes)?;
        file.sync_all()
    }

    /// Bytes currently in the log file.
    pub fn size(&self) -> io::Result<u64> {
        Ok(self.file.metadata()?.len())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Replay every intact record, in order.
    ///
    /// Returns the payloads up to (not including) the first torn or corrupt
    /// record. A missing file replays as an empty log — a fresh database.
    pub fn replay(path: impl AsRef<Path>) -> io::Result<ReplayResult> {
        let file = match File::open(path.as_ref()) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(ReplayResult::default())
            }
            Err(error) => return Err(error),
        };

        let file_len = file.metadata()?.len();
        let mut reader = BufReader::new(file);
        let mut records = Vec::new();
        let mut truncated = false;
        let mut valid_bytes: u64 = 0;

        loop {
            let mut header = [0u8; HEADER_LEN];
            match read_exact_or_eof(&mut reader, &mut header)? {
                ReadOutcome::Eof => break,
                // A partial header is itself a torn write.
                ReadOutcome::Partial => {
                    truncated = true;
                    break;
                }
                ReadOutcome::Full => {}
            }

            let len = u32::from_le_bytes(header[0..4].try_into().unwrap()) as usize;
            let expected_crc = u32::from_le_bytes(header[4..8].try_into().unwrap());

            let remaining = file_len.saturating_sub(valid_bytes + HEADER_LEN as u64);
            if len as u64 > remaining {
                truncated = true;
                break;
            }
            let mut payload = vec![0u8; len];
            match read_exact_or_eof(&mut reader, &mut payload)? {
                ReadOutcome::Full => {}
                // The length field promised more bytes than the file holds.
                _ => {
                    truncated = true;
                    break;
                }
            }

            if crc32(&payload) != expected_crc {
                truncated = true;
                break;
            }

            valid_bytes += (HEADER_LEN + len) as u64;
            records.push(payload);
        }

        Ok(ReplayResult {
            records,
            truncated,
            valid_bytes,
        })
    }
}

/// What a replay found.
#[derive(Default, Debug)]
pub struct ReplayResult {
    /// Payloads of every intact record, in write order.
    pub records: Vec<Vec<u8>>,
    /// True when replay stopped early because of a torn or corrupt record.
    pub truncated: bool,
    /// Byte length of the valid prefix — where a repair would truncate to.
    pub valid_bytes: u64,
}

enum ReadOutcome {
    Full,
    Partial,
    Eof,
}

/// Read exactly `buf.len()` bytes, distinguishing a clean EOF from a short read.
fn read_exact_or_eof(reader: &mut impl Read, buf: &mut [u8]) -> io::Result<ReadOutcome> {
    if buf.is_empty() {
        return Ok(ReadOutcome::Full);
    }
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(match filled {
        0 => ReadOutcome::Eof,
        n if n == buf.len() => ReadOutcome::Full,
        _ => ReadOutcome::Partial,
    })
}
