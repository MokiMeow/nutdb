//! End-to-end SQL tests over the durable MVCC engine.

use std::fs;
use std::path::PathBuf;

use nutdb::{SqlEngine, SqlResult, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("nutdb-sql-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    fn wal(&self) -> PathBuf {
        self.0.join("sql.wal")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn rows(result: &SqlResult) -> (&[String], &[Vec<Value>]) {
    match result {
        SqlResult::Rows { columns, rows } => (columns, rows),
        other => panic!("expected rows, got {other:?}"),
    }
}

#[test]
fn create_insert_select_order_limit_update_delete_and_reopen() {
    let dir = TempDir::new("end-to-end");
    let path = dir.wal();
    {
        let mut sql = SqlEngine::open(&path).unwrap();
        sql.execute(
            "CREATE TABLE users (
                id INT PRIMARY KEY,
                name TEXT,
                active BOOLEAN
            );
            INSERT INTO users (id, name, active) VALUES (3, 'Alan', TRUE);
            INSERT INTO users (id, name, active) VALUES (1, 'Ada', TRUE);
            INSERT INTO users (id, name, active) VALUES (2, 'Grace', FALSE);",
        )
        .unwrap();

        let result = sql
            .execute(
                "SELECT id, name FROM users
                 WHERE active = TRUE ORDER BY name DESC LIMIT 2;",
            )
            .unwrap();
        let (columns, rows) = rows(&result[0]);
        assert_eq!(columns, ["ID", "NAME"]);
        assert_eq!(
            rows,
            [
                vec![Value::Integer(3), Value::Text("Alan".into())],
                vec![Value::Integer(1), Value::Text("Ada".into())],
            ]
        );

        assert_eq!(
            sql.execute("UPDATE users SET active = TRUE WHERE id = 2;")
                .unwrap()[0],
            SqlResult::Affected(1)
        );
        assert_eq!(
            sql.execute("DELETE FROM users WHERE id = 3;").unwrap()[0],
            SqlResult::Affected(1)
        );
    }

    let mut reopened = SqlEngine::open(&path).unwrap();
    let result = reopened
        .execute("SELECT id, name FROM users ORDER BY id;")
        .unwrap();
    let (_, rows) = rows(&result[0]);
    assert_eq!(
        rows,
        [
            vec![Value::Integer(1), Value::Text("Ada".into())],
            vec![Value::Integer(2), Value::Text("Grace".into())],
        ]
    );
}

#[test]
fn begin_commit_and_rollback_control_visibility() {
    let dir = TempDir::new("transactions");
    let mut sql = SqlEngine::open(dir.wal()).unwrap();
    sql.execute("CREATE TABLE t (id INT PRIMARY KEY, value TEXT);")
        .unwrap();

    let results = sql
        .execute(
            "BEGIN;
             INSERT INTO t (id, value) VALUES (1, 'rolled back');
             ROLLBACK;",
        )
        .unwrap();
    assert_eq!(results[0], SqlResult::Begun);
    assert_eq!(results[2], SqlResult::RolledBack);
    assert!(rows(&sql.execute("SELECT * FROM t;").unwrap()[0]).1.is_empty());

    let results = sql
        .execute(
            "BEGIN;
             INSERT INTO t (id, value) VALUES (2, 'committed');
             COMMIT;",
        )
        .unwrap();
    assert_eq!(results[2], SqlResult::Committed);
    assert_eq!(rows(&sql.execute("SELECT * FROM t;").unwrap()[0]).1.len(), 1);
}

#[test]
fn null_uses_three_valued_logic() {
    let dir = TempDir::new("null");
    let mut sql = SqlEngine::open(dir.wal()).unwrap();
    sql.execute(
        "CREATE TABLE notes (id INT PRIMARY KEY, note TEXT, enabled BOOLEAN);
         INSERT INTO notes (id, note, enabled) VALUES (1, NULL, TRUE);
         INSERT INTO notes (id, note, enabled) VALUES (2, 'text', FALSE);",
    )
    .unwrap();

    for query in [
        "SELECT * FROM notes WHERE note = NULL;",
        "SELECT * FROM notes WHERE note != 'text';",
        "SELECT * FROM notes WHERE enabled = FALSE AND note = NULL;",
    ] {
        assert!(rows(&sql.execute(query).unwrap()[0]).1.is_empty());
    }
    let result = sql
        .execute("SELECT id FROM notes WHERE note = NULL OR id = 2;")
        .unwrap();
    assert_eq!(rows(&result[0]).1, [vec![Value::Integer(2)]]);
}

#[test]
fn explain_uses_primary_key_index_and_reports_full_scan() {
    let dir = TempDir::new("explain");
    let mut sql = SqlEngine::open(dir.wal()).unwrap();
    sql.execute("CREATE TABLE items (id INT PRIMARY KEY, name TEXT);")
        .unwrap();
    let indexed = sql
        .execute("EXPLAIN SELECT * FROM items WHERE id = 42;")
        .unwrap();
    let scanned = sql
        .execute("EXPLAIN SELECT * FROM items WHERE name = 'x';")
        .unwrap();
    assert!(matches!(&indexed[0], SqlResult::Explain(plan) if plan.contains("IndexLookup")));
    assert!(matches!(&scanned[0], SqlResult::Explain(plan) if plan.contains("Scan")));
}

#[test]
fn syntax_errors_include_the_source_position() {
    let dir = TempDir::new("errors");
    let mut sql = SqlEngine::open(dir.wal()).unwrap();
    let error = sql.execute("SELECT * FORM missing;").unwrap_err();
    let message = error.to_string();
    assert!(message.contains("byte 9"), "{message}");
    assert!(message.contains("expected"), "{message}");
}

#[test]
fn duplicate_primary_keys_and_wrong_types_are_rejected() {
    let dir = TempDir::new("validation");
    let mut sql = SqlEngine::open(dir.wal()).unwrap();
    sql.execute("CREATE TABLE t (id INT PRIMARY KEY, name TEXT);")
        .unwrap();
    sql.execute("INSERT INTO t (id, name) VALUES (1, 'ok');")
        .unwrap();
    assert!(sql
        .execute("INSERT INTO t (id, name) VALUES (1, 'duplicate');")
        .is_err());
    assert!(sql
        .execute("INSERT INTO t (id, name) VALUES ('wrong', 'type');")
        .is_err());
}
