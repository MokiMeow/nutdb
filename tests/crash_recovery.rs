//! Crash-recovery tests — the proof that milestone 0 actually works.
//!
//! Dropping a `Store` and reopening it is exactly what a process crash looks
//! like to the data: nothing but the log file survives. These tests also
//! simulate *torn writes* by truncating and corrupting the log, because the
//! interesting failure is not a clean shutdown — it is a crash halfway through
//! an append.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use nutdb::{Store, Wal};

/// A temp directory that cleans itself up, so tests never share state.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!("nutdb-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    fn wal(&self) -> PathBuf {
        self.0.join("nutdb.wal")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn writes_survive_a_simulated_crash() {
    let dir = TempDir::new("survive");
    let path = dir.wal();

    {
        let mut store = Store::open(&path).unwrap();
        store.set("user:1", "ada").unwrap();
        store.set("user:2", "grace").unwrap();
        store.set("user:3", "alan").unwrap();
    } // drop == crash

    let store = Store::open(&path).unwrap();
    assert_eq!(store.get("user:1"), Some("ada"));
    assert_eq!(store.get("user:2"), Some("grace"));
    assert_eq!(store.get("user:3"), Some("alan"));
    assert_eq!(store.len(), 3);
    assert!(!store.recovery().truncated);
    assert_eq!(store.recovery().records_replayed, 3);
}

#[test]
fn last_write_wins_and_deletes_persist() {
    let dir = TempDir::new("overwrite");
    let path = dir.wal();

    {
        let mut store = Store::open(&path).unwrap();
        store.set("k", "first").unwrap();
        store.set("k", "second").unwrap();
        store.set("doomed", "x").unwrap();
        store.delete("doomed").unwrap();
    }

    let store = Store::open(&path).unwrap();
    assert_eq!(store.get("k"), Some("second"));
    assert!(!store.contains("doomed"));
    assert_eq!(store.len(), 1);
}

#[test]
fn survives_many_reopens() {
    let dir = TempDir::new("reopen");
    let path = dir.wal();

    for i in 0..25 {
        let mut store = Store::open(&path).unwrap();
        store.set(format!("key:{i}"), format!("value:{i}")).unwrap();
    }

    let store = Store::open(&path).unwrap();
    assert_eq!(store.len(), 25);
    for i in 0..25 {
        assert_eq!(
            store.get(&format!("key:{i}")),
            Some(format!("value:{i}").as_str())
        );
    }
}

#[test]
fn torn_write_is_discarded_and_earlier_writes_survive() {
    let dir = TempDir::new("torn");
    let path = dir.wal();

    {
        let mut store = Store::open(&path).unwrap();
        store.set("safe:1", "one").unwrap();
        store.set("safe:2", "two").unwrap();
    }

    let good_len = fs::metadata(&path).unwrap().len();

    // Simulate a crash mid-append: a header promising a payload that was never
    // written. This is precisely the shape of a real torn write.
    {
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(&999u32.to_le_bytes()).unwrap(); // len
        file.write_all(&0xDEAD_BEEFu32.to_le_bytes()).unwrap(); // crc
        file.write_all(b"partial payload, cut off").unwrap();
        file.sync_all().unwrap();
    }

    let store = Store::open(&path).unwrap();
    assert_eq!(store.get("safe:1"), Some("one"), "committed data must survive");
    assert_eq!(store.get("safe:2"), Some("two"));
    assert_eq!(store.len(), 2, "the torn record must not appear");

    let recovery = store.recovery();
    assert!(recovery.truncated, "recovery must report the torn tail");
    assert_eq!(
        recovery.valid_bytes, good_len,
        "valid prefix must end exactly where the good records ended"
    );
}

#[test]
fn corrupted_payload_fails_its_checksum() {
    let dir = TempDir::new("corrupt");
    let path = dir.wal();

    {
        let mut store = Store::open(&path).unwrap();
        store.set("first", "intact").unwrap();
        store.set("second", "will-be-corrupted").unwrap();
    }

    // Flip a bit inside the *last* record's payload. Without a checksum this
    // would be replayed as if it were real data.
    let mut bytes = fs::read(&path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0b0000_0001;
    fs::write(&path, &bytes).unwrap();

    let store = Store::open(&path).unwrap();
    assert_eq!(store.get("first"), Some("intact"));
    assert!(
        !store.contains("second"),
        "a record failing its CRC must not be applied"
    );
    assert!(store.recovery().truncated);
}

#[test]
fn truncated_header_is_handled() {
    let dir = TempDir::new("short-header");
    let path = dir.wal();

    {
        let mut store = Store::open(&path).unwrap();
        store.set("a", "1").unwrap();
    }

    // Three stray bytes: less than a full 8-byte header.
    {
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(&[0x01, 0x02, 0x03]).unwrap();
        file.sync_all().unwrap();
    }

    let store = Store::open(&path).unwrap();
    assert_eq!(store.get("a"), Some("1"));
    assert!(store.recovery().truncated);
}

#[test]
fn missing_log_opens_as_an_empty_database() {
    let dir = TempDir::new("missing");
    let store = Store::open(dir.0.join("does-not-exist.wal")).unwrap();
    assert!(store.is_empty());
    assert!(!store.recovery().truncated);
}

#[test]
fn empty_log_file_replays_cleanly() {
    let dir = TempDir::new("empty");
    let path = dir.wal();
    fs::write(&path, b"").unwrap();

    let store = Store::open(&path).unwrap();
    assert!(store.is_empty());
    assert!(!store.recovery().truncated);
}

#[test]
fn wal_replays_records_in_write_order() {
    let dir = TempDir::new("order");
    let path = dir.wal();

    {
        let mut wal = Wal::open(&path).unwrap();
        for i in 0..100u32 {
            wal.append(&i.to_le_bytes()).unwrap();
        }
        wal.sync().unwrap();
    }

    let replayed = Wal::replay(&path).unwrap();
    assert_eq!(replayed.records.len(), 100);
    assert!(!replayed.truncated);
    for (i, record) in replayed.records.iter().enumerate() {
        assert_eq!(record.as_slice(), &(i as u32).to_le_bytes());
    }
}

#[test]
fn handles_empty_and_large_payloads() {
    let dir = TempDir::new("sizes");
    let path = dir.wal();
    let large = vec![0xABu8; 256 * 1024];

    {
        let mut wal = Wal::open(&path).unwrap();
        wal.append(b"").unwrap();
        wal.append(&large).unwrap();
        wal.append(b"tail").unwrap();
        wal.sync().unwrap();
    }

    let replayed = Wal::replay(&path).unwrap();
    assert_eq!(replayed.records.len(), 3);
    assert!(replayed.records[0].is_empty());
    assert_eq!(replayed.records[1], large);
    assert_eq!(replayed.records[2], b"tail");
}
