//! Real TCP key/value nodes used by the cluster fault harness.
//!
//! Consensus safety is implemented and tested in [`crate::raft`]. This server
//! is the integration layer: it discovers reachable peers, deterministically
//! redirects to one leader in each connected component, refuses minority
//! writes, synchronously replicates before acknowledging, and merges durable
//! versioned records when a node restarts.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io;
use std::net::{SocketAddr, TcpListener};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::raft::transport::{read_frame, write_frame};
use crate::Store;

#[derive(Clone, Debug)]
struct ReplicaValue {
    version: u64,
    tombstone: bool,
    committed: bool,
    value: String,
}

pub struct Server {
    id: u64,
    listener: TcpListener,
    peers: BTreeMap<u64, SocketAddr>,
    blocked: HashSet<u64>,
    store: Store,
}

impl Server {
    pub fn bind(
        id: u64,
        address: SocketAddr,
        peers: BTreeMap<u64, SocketAddr>,
        data_dir: impl AsRef<Path>,
    ) -> io::Result<Self> {
        fs::create_dir_all(data_dir.as_ref())?;
        let listener = TcpListener::bind(address)?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            id,
            listener,
            peers,
            blocked: HashSet::new(),
            store: Store::open(data_dir.as_ref().join("cluster.wal"))?,
        })
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    pub fn run(mut self, running: Arc<AtomicBool>) -> io::Result<()> {
        while running.load(Ordering::SeqCst) {
            match self.listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
                    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
                    let response = match read_frame(&mut stream)
                        .and_then(|request| self.handle(&request))
                    {
                        Ok(response) => response,
                        Err(error) => format!("ERROR {error}"),
                    };
                    let _ = write_frame(&mut stream, response.as_bytes());
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    fn handle(&mut self, request: &[u8]) -> io::Result<String> {
        let request = std::str::from_utf8(request)
            .map_err(|_| invalid("cluster: request is not UTF-8"))?;
        let parts: Vec<&str> = request.split_whitespace().collect();
        match parts.as_slice() {
            ["PING"] => Ok(format!("PONG {}", self.id)),
            ["DUMP"] => Ok(format!("DUMP {}", self.dump()?)),
            ["REPL", key, version, tombstone, value] => {
                let key = unhex_string(key)?;
                let value = unhex_string(value)?;
                let version = parse_u64(version)?;
                let tombstone = parse_bool(tombstone)?;
                self.merge(
                    key,
                    ReplicaValue {
                        version,
                        tombstone,
                        committed: false,
                        value,
                    },
                )?;
                Ok("OK".to_owned())
            }
            ["COMMIT", key, version] => {
                let key = unhex_string(key)?;
                let version = parse_u64(version)?;
                if let Some(mut record) = self.read_record(&key)? {
                    if record.version == version {
                        record.committed = true;
                        self.store.set(key, encode_record(&record))?;
                    }
                }
                Ok("OK".to_owned())
            }
            ["PUT", key, value] => {
                let key = unhex_string(key)?;
                let value = unhex_string(value)?;
                self.client_write(key, value, false)
            }
            ["DELETE", key] => {
                let key = unhex_string(key)?;
                self.client_write(key, String::new(), true)
            }
            ["GET", key] => {
                let key = unhex_string(key)?;
                self.client_read(&key)
            }
            ["ADMIN_BLOCK", peer] => {
                self.blocked.insert(parse_u64(peer)?);
                Ok("OK".to_owned())
            }
            ["ADMIN_HEAL"] => {
                self.blocked.clear();
                Ok("OK".to_owned())
            }
            _ => Err(invalid("cluster: unknown request")),
        }
    }

    fn client_write(&mut self, key: String, value: String, tombstone: bool) -> io::Result<String> {
        let alive = self.alive_peers();
        let leader = alive
            .iter()
            .copied()
            .chain(std::iter::once(self.id))
            .min()
            .expect("self present");
        if leader != self.id {
            return Ok(format!("REDIRECT {leader}"));
        }
        if alive.len() + 1 <= self.peers.len() / 2 {
            return Ok("RETRY no-quorum".to_owned());
        }

        self.catch_up(&alive)?;
        let version = self
            .read_record(&key)?
            .map(|record| record.version)
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| invalid("cluster: version exhausted"))?;
        let record = ReplicaValue {
            version,
            tombstone,
            committed: false,
            value,
        };
        let mut staged = Vec::new();
        for peer in &alive {
            let request = format!(
                "REPL {} {} {} {}",
                hex(key.as_bytes()),
                record.version,
                u8::from(record.tombstone),
                hex(record.value.as_bytes())
            );
            if request_peer_fast(self.peers[peer], &request)
                .map(|response| response == "OK")
                .unwrap_or(false)
            {
                staged.push(*peer);
            }
        }
        if (staged.len() + 1) * 2 <= self.peers.len() + 1 {
            return Ok("RETRY indeterminate".to_owned());
        }
        let mut committed = record;
        committed.committed = true;
        self.merge(key.clone(), committed)?;
        let mut acknowledgements = 1;
        for peer in staged {
            let request = format!("COMMIT {} {}", hex(key.as_bytes()), version);
            if request_peer_fast(self.peers[&peer], &request)
                .map(|response| response == "OK")
                .unwrap_or(false)
            {
                acknowledgements += 1;
            }
        }
        if acknowledgements * 2 > self.peers.len() + 1 {
            Ok(format!("OK {version}"))
        } else {
            Ok("RETRY indeterminate".to_owned())
        }
    }

    fn client_read(&mut self, key: &str) -> io::Result<String> {
        let alive = self.alive_peers();
        let leader = alive
            .iter()
            .copied()
            .chain(std::iter::once(self.id))
            .min()
            .expect("self present");
        if leader != self.id {
            return Ok(format!("REDIRECT {leader}"));
        }
        if alive.len() + 1 <= self.peers.len() / 2 {
            return Ok("RETRY no-quorum".to_owned());
        }
        self.catch_up(&alive)?;
        match self.read_record(key)? {
            Some(record) if record.committed && !record.tombstone => {
                Ok(format!("VALUE {}", hex(record.value.as_bytes())))
            }
            _ => Ok("NIL".to_owned()),
        }
    }

    fn alive_peers(&self) -> Vec<u64> {
        self.peers
            .iter()
            .filter(|(id, _)| !self.blocked.contains(id))
            .filter_map(|(id, address)| {
                request_peer_fast(*address, "PING")
                    .ok()
                    .filter(|response| response == &format!("PONG {id}"))
                    .map(|_| *id)
            })
            .collect()
    }

    fn catch_up(&mut self, peers: &[u64]) -> io::Result<()> {
        for peer in peers {
            let Ok(response) = request_peer_fast(self.peers[peer], "DUMP") else {
                continue;
            };
            let Some(dump) = response.strip_prefix("DUMP ") else {
                continue;
            };
            for (key, record) in decode_dump(dump)? {
                self.merge(key, record)?;
            }
        }
        Ok(())
    }

    fn dump(&self) -> io::Result<String> {
        let mut rows = Vec::new();
        for key in self.store.keys_sorted() {
            let record = self
                .read_record(key)?
                .ok_or_else(|| invalid("cluster: missing stored record"))?;
            if !record.committed {
                continue;
            }
            rows.push(format!(
                "{},{},{},{},{}",
                hex(key.as_bytes()),
                record.version,
                u8::from(record.tombstone),
                u8::from(record.committed),
                hex(record.value.as_bytes())
            ));
        }
        Ok(rows.join(";"))
    }

    fn merge(&mut self, key: String, incoming: ReplicaValue) -> io::Result<()> {
        let replace = self
            .read_record(&key)?
            .map(|current| {
                incoming.version > current.version
                    || (incoming.version == current.version
                        && incoming.committed
                        && !current.committed)
            })
            .unwrap_or(true);
        if replace {
            self.store.set(key, encode_record(&incoming))?;
        }
        Ok(())
    }

    fn read_record(&self, key: &str) -> io::Result<Option<ReplicaValue>> {
        self.store.get(key).map(decode_record).transpose()
    }
}

pub fn request_peer(address: SocketAddr, request: &str) -> io::Result<String> {
    request_peer_with_timeout(address, request, Duration::from_secs(2))
}

fn request_peer_fast(address: SocketAddr, request: &str) -> io::Result<String> {
    request_peer_with_timeout(address, request, Duration::from_millis(100))
}

fn request_peer_with_timeout(
    address: SocketAddr,
    request: &str,
    timeout: Duration,
) -> io::Result<String> {
    let mut stream = std::net::TcpStream::connect_timeout(&address, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    write_frame(&mut stream, request.as_bytes())?;
    let response = read_frame(&mut stream)?;
    String::from_utf8(response).map_err(|_| invalid("cluster: response is not UTF-8"))
}

fn encode_record(record: &ReplicaValue) -> String {
    format!(
        "{}:{}:{}:{}",
        record.version,
        u8::from(record.tombstone),
        u8::from(record.committed),
        hex(record.value.as_bytes())
    )
}

fn decode_record(encoded: &str) -> io::Result<ReplicaValue> {
    let mut parts = encoded.splitn(4, ':');
    let version = parse_u64(parts.next().ok_or_else(|| invalid("cluster: bad record"))?)?;
    let tombstone = parse_bool(parts.next().ok_or_else(|| invalid("cluster: bad record"))?)?;
    let committed = parse_bool(parts.next().ok_or_else(|| invalid("cluster: bad record"))?)?;
    let value = unhex_string(parts.next().ok_or_else(|| invalid("cluster: bad record"))?)?;
    Ok(ReplicaValue {
        version,
        tombstone,
        committed,
        value,
    })
}

fn decode_dump(dump: &str) -> io::Result<Vec<(String, ReplicaValue)>> {
    if dump.is_empty() {
        return Ok(Vec::new());
    }
    dump.split(';')
        .map(|row| {
            let fields: Vec<&str> = row.split(',').collect();
            if fields.len() != 5 {
                return Err(invalid("cluster: malformed dump"));
            }
            Ok((
                unhex_string(fields[0])?,
                ReplicaValue {
                    version: parse_u64(fields[1])?,
                    tombstone: parse_bool(fields[2])?,
                    committed: parse_bool(fields[3])?,
                    value: unhex_string(fields[4])?,
                },
            ))
        })
        .collect()
}

fn parse_u64(value: &str) -> io::Result<u64> {
    value
        .parse()
        .map_err(|_| invalid("cluster: expected unsigned integer"))
}

fn parse_bool(value: &str) -> io::Result<bool> {
    match value {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(invalid("cluster: expected 0 or 1")),
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 15) as usize] as char);
    }
    out
}

fn unhex_string(value: &str) -> io::Result<String> {
    if value.len() % 2 != 0 {
        return Err(invalid("cluster: odd hex length"));
    }
    let bytes: io::Result<Vec<u8>> = value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| Ok((digit(pair[0])? << 4) | digit(pair[1])?))
        .collect();
    String::from_utf8(bytes?).map_err(|_| invalid("cluster: value is not UTF-8"))
}

fn digit(byte: u8) -> io::Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(invalid("cluster: invalid hex")),
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
