//! nutdb demo: show durability by writing, reopening, and reporting recovery.
//!
//!   cargo run -- demo          write some keys, reopen, prove they survived
//!   cargo run -- set k v       set a key in data/nutdb.wal
//!   cargo run -- get k         read a key back
//!   cargo run -- list          show every key

use std::io;
use std::process::ExitCode;

use nutdb::Store;

const DEFAULT_PATH: &str = "data/nutdb.wal";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("demo");

    let result = match command {
        "demo" => demo(),
        "set" => match args.get(1..3) {
            Some([key, value]) => set(key, value),
            _ => usage("set <key> <value>"),
        },
        "get" => match args.get(1) {
            Some(key) => get(key),
            None => usage("get <key>"),
        },
        "list" => list(),
        other => usage(&format!("unknown command '{other}'")),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("nutdb: {error}");
            ExitCode::FAILURE
        }
    }
}

fn demo() -> io::Result<()> {
    let path = "data/demo.wal";
    let _ = std::fs::remove_file(path);

    println!("nutdb milestone 0 — write-ahead log durability\n");

    {
        let mut store = Store::open(path)?;
        println!("opening a fresh store at {path}");
        for (key, value) in [
            ("user:1", "ada"),
            ("user:2", "grace"),
            ("user:3", "alan"),
            ("temp", "scratch"),
        ] {
            store.set(key, value)?;
            println!("  set {key} = {value}");
        }
        store.delete("temp")?;
        println!("  delete temp");
        println!(
            "  {} keys live, log is {} bytes",
            store.len(),
            store.wal_size()?
        );
    } // the Store is dropped here — as if the process had exited

    println!("\nprocess 'crashes' — reopening from the log only");

    let store = Store::open(path)?;
    let recovery = store.recovery();
    println!(
        "  replayed {} records (truncated: {})",
        recovery.records_replayed, recovery.truncated
    );
    println!("  keys recovered: {:?}", store.keys_sorted());

    let ok = store.get("user:1") == Some("ada")
        && store.get("user:2") == Some("grace")
        && store.get("user:3") == Some("alan")
        && !store.contains("temp");

    println!(
        "\n{}",
        if ok {
            "durability verified: every committed write survived, the delete stuck"
        } else {
            "FAILED: recovered state does not match what was committed"
        }
    );

    if ok {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "recovery mismatch",
        ))
    }
}

fn set(key: &str, value: &str) -> io::Result<()> {
    let mut store = Store::open(DEFAULT_PATH)?;
    store.set(key, value)?;
    println!("set {key} = {value}");
    Ok(())
}

fn get(key: &str) -> io::Result<()> {
    let store = Store::open(DEFAULT_PATH)?;
    match store.get(key) {
        Some(value) => println!("{value}"),
        None => println!("(nil)"),
    }
    Ok(())
}

fn list() -> io::Result<()> {
    let store = Store::open(DEFAULT_PATH)?;
    for key in store.keys_sorted() {
        println!("{key} = {}", store.get(key).unwrap_or(""));
    }
    Ok(())
}

fn usage(message: &str) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("usage: nutdb [demo | set <k> <v> | get <k> | list]  ({message})"),
    ))
}
