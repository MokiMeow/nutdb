//! Controllable in-memory delivery and a length-prefixed TCP frame.

use std::collections::{HashSet, VecDeque};
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};

const MAX_FRAME: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub from: u64,
    pub to: u64,
    pub payload: Vec<u8>,
}

#[derive(Default)]
pub struct InMemoryTransport {
    blocked: HashSet<(u64, u64)>,
    queue: VecDeque<Message>,
}

impl InMemoryTransport {
    pub fn reachable(&self, from: u64, to: u64) -> bool {
        !self.blocked.contains(&(from, to))
    }

    pub fn partition(&mut self, a: u64, b: u64) {
        self.blocked.insert((a, b));
        self.blocked.insert((b, a));
    }

    pub fn heal(&mut self, a: u64, b: u64) {
        self.blocked.remove(&(a, b));
        self.blocked.remove(&(b, a));
    }

    pub fn heal_all(&mut self) {
        self.blocked.clear();
    }

    pub fn send(&mut self, message: Message) -> bool {
        if !self.reachable(message.from, message.to) {
            return false;
        }
        self.queue.push_back(message);
        true
    }

    pub fn deliver_next(&mut self) -> Option<Message> {
        self.queue.pop_front()
    }

    pub fn deliver_last(&mut self) -> Option<Message> {
        self.queue.pop_back()
    }

    pub fn drop_next(&mut self) -> Option<Message> {
        self.queue.pop_front()
    }
}

pub struct TcpTransport;

impl TcpTransport {
    pub fn send(address: impl ToSocketAddrs, payload: &[u8]) -> io::Result<Vec<u8>> {
        let mut stream = TcpStream::connect(address)?;
        write_frame(&mut stream, payload)?;
        read_frame(&mut stream)
    }
}

pub fn write_frame(writer: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    let len = u32::try_from(payload.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "raft: frame too large"))?;
    if payload.len() > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "raft: frame exceeds limit",
        ));
    }
    writer.write_all(&len.to_be_bytes())?;
    writer.write_all(payload)?;
    writer.flush()
}

pub fn read_frame(reader: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut header = [0u8; 4];
    reader.read_exact(&mut header)?;
    let len = u32::from_be_bytes(header) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "raft: incoming frame exceeds limit",
        ));
    }
    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload)?;
    Ok(payload)
}
