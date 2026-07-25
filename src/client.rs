//! Cluster client with leader redirects and bounded retry.

use std::collections::BTreeMap;
use std::io;
use std::net::SocketAddr;
use std::thread;
use std::time::Duration;

use crate::server::request_peer;

#[derive(Clone)]
pub struct ClusterClient {
    nodes: BTreeMap<u64, SocketAddr>,
    attempts: usize,
}

impl ClusterClient {
    pub fn new(nodes: BTreeMap<u64, SocketAddr>) -> Self {
        Self { nodes, attempts: 8 }
    }

    pub fn put(&self, key: &str, value: &str) -> io::Result<()> {
        let response = self.request(&format!(
            "PUT {} {}",
            hex(key.as_bytes()),
            hex(value.as_bytes())
        ))?;
        if response.starts_with("OK ") {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "cluster: write remained indeterminate",
            ))
        }
    }

    pub fn delete(&self, key: &str) -> io::Result<()> {
        let response = self.request(&format!("DELETE {}", hex(key.as_bytes())))?;
        if response.starts_with("OK ") {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "cluster: delete remained indeterminate",
            ))
        }
    }

    pub fn get(&self, key: &str) -> io::Result<Option<String>> {
        let response = self.request(&format!("GET {}", hex(key.as_bytes())))?;
        if response == "NIL" {
            Ok(None)
        } else if let Some(value) = response.strip_prefix("VALUE ") {
            Ok(Some(unhex_string(value)?))
        } else {
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "cluster: read remained indeterminate",
            ))
        }
    }

    pub fn block(&self, node: u64, peer: u64) -> io::Result<()> {
        let address = self
            .nodes
            .get(&node)
            .ok_or_else(|| invalid("cluster: unknown node"))?;
        let response = request_peer(*address, &format!("ADMIN_BLOCK {peer}"))?;
        if response == "OK" {
            Ok(())
        } else {
            Err(invalid("cluster: block failed"))
        }
    }

    pub fn heal(&self, node: u64) -> io::Result<()> {
        let address = self
            .nodes
            .get(&node)
            .ok_or_else(|| invalid("cluster: unknown node"))?;
        let response = request_peer(*address, "ADMIN_HEAL")?;
        if response == "OK" {
            Ok(())
        } else {
            Err(invalid("cluster: heal failed"))
        }
    }

    fn request(&self, request: &str) -> io::Result<String> {
        let mut preferred = self.nodes.keys().copied().next();
        let mut delay = 5;
        for _ in 0..self.attempts {
            let ids: Vec<u64> = preferred
                .into_iter()
                .chain(self.nodes.keys().copied().filter(|id| Some(*id) != preferred))
                .collect();
            for id in ids {
                let Ok(response) = request_peer(self.nodes[&id], request) else {
                    continue;
                };
                if let Some(target) = response.strip_prefix("REDIRECT ") {
                    preferred = target.parse().ok();
                    break;
                }
                if response.starts_with("RETRY ") {
                    continue;
                }
                return Ok(response);
            }
            thread::sleep(Duration::from_millis(delay));
            delay = (delay * 2).min(100);
        }
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "cluster: no majority leader answered",
        ))
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
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
