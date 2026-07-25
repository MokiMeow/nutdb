//! nutdb demo: show durability by writing, reopening, and reporting recovery.
//!
//!   cargo run -- demo          write some keys, reopen, prove they survived
//!   cargo run -- set k v       set a key in data/nutdb.wal
//!   cargo run -- get k         read a key back
//!   cargo run -- list          show every key
//!   cargo run -- sql "..."     execute SQL against data/nutdb.sql.wal

use std::collections::BTreeMap;
use std::io;
use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use nutdb::server::Server;
use nutdb::{ClusterClient, SqlEngine, SqlResult, Store};

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
        "sql" => match args.get(1) {
            Some(source) => sql(source),
            None => usage("sql <statement(s)>"),
        },
        "serve" => serve(&args[1..]),
        "client" => cluster_client(&args[1..]),
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

fn sql(source: &str) -> io::Result<()> {
    let mut engine = SqlEngine::open("data/nutdb.sql.wal")?;
    for result in engine.execute(source)? {
        match result {
            SqlResult::Affected(count) => println!("{count} row(s) affected"),
            SqlResult::Rows { columns, rows } => {
                println!("{}", columns.join(" | "));
                for row in rows {
                    println!(
                        "{}",
                        row.iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(" | ")
                    );
                }
            }
            SqlResult::Begun => println!("BEGIN"),
            SqlResult::Committed => println!("COMMIT"),
            SqlResult::RolledBack => println!("ROLLBACK"),
            SqlResult::Explain(plan) => println!("{plan}"),
        }
    }
    Ok(())
}

fn serve(args: &[String]) -> io::Result<()> {
    let id: u64 = flag(args, "--id")?
        .parse()
        .map_err(|_| invalid_argument("serve: --id must be an integer"))?;
    let listen: SocketAddr = flag(args, "--listen")?
        .parse()
        .map_err(|_| invalid_argument("serve: invalid --listen address"))?;
    let data = flag(args, "--data")?;
    let mut peers = BTreeMap::new();
    let peer_list = flag(args, "--peers")?;
    if !peer_list.is_empty() {
        for peer in peer_list.split(',') {
            let (peer_id, address) = peer
                .split_once('=')
                .ok_or_else(|| invalid_argument("serve: peers are id=address"))?;
            peers.insert(
                peer_id
                    .parse()
                    .map_err(|_| invalid_argument("serve: invalid peer id"))?,
                address
                    .parse()
                    .map_err(|_| invalid_argument("serve: invalid peer address"))?,
            );
        }
    }
    let server = Server::bind(id, listen, peers, data)?;
    println!("node {id} listening on {}", server.local_addr()?);
    server.run(Arc::new(AtomicBool::new(true)))
}

fn cluster_client(args: &[String]) -> io::Result<()> {
    let mut nodes = BTreeMap::new();
    for node in flag(args, "--nodes")?.split(',') {
        let (id, address) = node
            .split_once('=')
            .ok_or_else(|| invalid_argument("client: nodes are id=address"))?;
        nodes.insert(
            id.parse()
                .map_err(|_| invalid_argument("client: invalid node id"))?,
            address
                .parse()
                .map_err(|_| invalid_argument("client: invalid node address"))?,
        );
    }
    let command_at = args
        .iter()
        .position(|arg| matches!(arg.as_str(), "put" | "get" | "delete"))
        .ok_or_else(|| invalid_argument("client: expected put/get/delete"))?;
    let client = ClusterClient::new(nodes);
    match args[command_at].as_str() {
        "put" => {
            let key = args
                .get(command_at + 1)
                .ok_or_else(|| invalid_argument("client put: missing key"))?;
            let value = args
                .get(command_at + 2)
                .ok_or_else(|| invalid_argument("client put: missing value"))?;
            client.put(key, value)?;
            println!("ok");
        }
        "get" => {
            let key = args
                .get(command_at + 1)
                .ok_or_else(|| invalid_argument("client get: missing key"))?;
            println!("{}", client.get(key)?.as_deref().unwrap_or("(nil)"));
        }
        "delete" => {
            let key = args
                .get(command_at + 1)
                .ok_or_else(|| invalid_argument("client delete: missing key"))?;
            client.delete(key)?;
            println!("ok");
        }
        _ => unreachable!("position filter"),
    }
    Ok(())
}

fn flag<'a>(args: &'a [String], name: &str) -> io::Result<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
        .ok_or_else(|| invalid_argument(&format!("serve: missing {name}")))
}

fn invalid_argument(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn usage(message: &str) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
            "usage: nutdb [demo | set/get/list | sql | serve | client --nodes ID=ADDR,... put/get/delete]  ({message})"
        ),
    ))
}
