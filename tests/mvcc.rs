//! Milestone 2: snapshots, conflicts, recovery, and watermark-safe GC.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Barrier};

use nutdb::{MvccStore, TxnError};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("nutdb-mvcc-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    fn wal(&self) -> PathBuf {
        self.0.join("mvcc.wal")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn long_reader_keeps_a_stable_snapshot_without_phantoms() {
    let dir = TempDir::new("snapshot");
    let store = MvccStore::open(dir.wal()).unwrap();
    let mut seed = store.begin().unwrap();
    seed.set("account", "100").unwrap();
    seed.commit().unwrap();

    let reader = store.begin().unwrap();
    let mut writer = store.begin().unwrap();
    writer.set("account", "150").unwrap();
    writer.set("new-row", "visible-later").unwrap();
    writer.commit().unwrap();

    assert_eq!(reader.get("account").unwrap().as_deref(), Some("100"));
    assert_eq!(reader.get("new-row").unwrap(), None);

    let current = store.begin().unwrap();
    assert_eq!(current.get("account").unwrap().as_deref(), Some("150"));
    assert_eq!(
        current.get("new-row").unwrap().as_deref(),
        Some("visible-later")
    );
}

#[test]
fn concurrent_writers_conflict_exactly_once() {
    let dir = TempDir::new("conflict");
    let store = MvccStore::open(dir.wal()).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let mut handles = Vec::new();
    for value in ["left", "right"] {
        let store = store.clone();
        let barrier = barrier.clone();
        handles.push(std::thread::spawn(move || {
            let mut txn = store.begin().unwrap();
            txn.set("same-key", value).unwrap();
            barrier.wait();
            txn.commit()
        }));
    }

    let results: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(TxnError::Conflict { .. })))
            .count(),
        1
    );
}

#[test]
fn uncommitted_writes_disappear_and_transaction_ids_do_not_repeat() {
    let dir = TempDir::new("recovery");
    let path = dir.wal();
    let first_id;
    {
        let store = MvccStore::open(&path).unwrap();
        let mut txn = store.begin().unwrap();
        first_id = txn.id();
        txn.set("partial", "must-not-appear").unwrap();
        // Dropping here models a crash after write records but before commit.
    }

    let store = MvccStore::open(&path).unwrap();
    let next = store.begin().unwrap();
    assert!(next.id() > first_id);
    assert_eq!(next.get("partial").unwrap(), None);
}

#[test]
fn committed_batches_recover_atomically() {
    let dir = TempDir::new("committed-recovery");
    let path = dir.wal();
    {
        let store = MvccStore::open(&path).unwrap();
        let mut txn = store.begin().unwrap();
        txn.set("a", "1").unwrap();
        txn.set("b", "2").unwrap();
        txn.commit().unwrap();
    }
    let store = MvccStore::open(&path).unwrap();
    let reader = store.begin().unwrap();
    assert_eq!(reader.get("a").unwrap().as_deref(), Some("1"));
    assert_eq!(reader.get("b").unwrap().as_deref(), Some("2"));
}

#[test]
fn gc_never_reclaims_a_version_visible_to_a_live_reader() {
    let dir = TempDir::new("gc");
    let store = MvccStore::open(dir.wal()).unwrap();
    let mut first = store.begin().unwrap();
    first.set("k", "v1").unwrap();
    first.commit().unwrap();

    let old_reader = store.begin().unwrap();
    let mut second = store.begin().unwrap();
    second.set("k", "v2").unwrap();
    second.commit().unwrap();

    let held = store.gc().unwrap();
    assert_eq!(held.versions_reclaimed, 0);
    assert_eq!(store.version_count("k").unwrap(), 2);
    assert_eq!(old_reader.get("k").unwrap().as_deref(), Some("v1"));
    drop(old_reader);

    let reclaimed = store.gc().unwrap();
    assert_eq!(reclaimed.versions_reclaimed, 1);
    assert_eq!(store.version_count("k").unwrap(), 1);
}

#[test]
fn snapshot_uses_commit_order_not_transaction_id_order() {
    let dir = TempDir::new("commit-order");
    let store = MvccStore::open(dir.wal()).unwrap();
    let mut older = store.begin().unwrap();
    let mut newer = store.begin().unwrap();
    newer.set("first-commit", "yes").unwrap();
    newer.commit().unwrap();

    let observer = store.begin().unwrap();
    older.set("late-commit", "hidden").unwrap();
    older.commit().unwrap();

    assert_eq!(observer.get("first-commit").unwrap().as_deref(), Some("yes"));
    assert_eq!(observer.get("late-commit").unwrap(), None);
}

#[test]
fn deletes_are_versioned_and_abort_leaves_no_trace() {
    let dir = TempDir::new("delete-abort");
    let store = MvccStore::open(dir.wal()).unwrap();
    let mut seed = store.begin().unwrap();
    seed.set("keep-history", "old").unwrap();
    seed.commit().unwrap();

    let old_reader = store.begin().unwrap();
    let mut deleting = store.begin().unwrap();
    deleting.delete("keep-history").unwrap();
    deleting.commit().unwrap();
    assert_eq!(old_reader.get("keep-history").unwrap().as_deref(), Some("old"));
    let current = store.begin().unwrap();
    assert_eq!(current.get("keep-history").unwrap(), None);

    let mut aborted = store.begin().unwrap();
    aborted.set("never", "visible").unwrap();
    aborted.abort().unwrap();
    let after_abort = store.begin().unwrap();
    assert_eq!(after_abort.get("never").unwrap(), None);
}
