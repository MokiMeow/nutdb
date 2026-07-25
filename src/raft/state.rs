//! Persistent Raft state and Figure 2 RPC rules.
//!
//! `current_term`, `voted_for`, and the log are serialized as a complete
//! checksummed WAL record before a vote is granted or an AppendEntries success
//! is returned. Volatile commit/apply indices are reconstructed by the leader.

use std::io;
use std::path::Path;

use crate::wal::{ReplayResult, Wal};

use super::rpc::{
    AppendEntries, AppendResponse, Entry, RequestVote, VoteResponse,
};

const STATE_TAG: u8 = 0x52;
const STATE_VERSION: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Follower,
    Candidate,
    Leader,
}

pub struct RaftNode {
    id: u64,
    wal: Wal,
    current_term: u64,
    voted_for: Option<u64>,
    log: Vec<Entry>,
    commit_index: u64,
    last_applied: u64,
    role: Role,
    leader_id: Option<u64>,
    election_timeout_ms: u64,
}

impl RaftNode {
    pub fn open(id: u64, path: impl AsRef<Path>, seed: u64) -> io::Result<Self> {
        let path = path.as_ref();
        let ReplayResult {
            records,
            truncated,
            valid_bytes,
        } = Wal::replay(path)?;
        let mut state = PersistentState::default();
        for record in records {
            state = PersistentState::decode(&record)?;
        }
        if truncated {
            Wal::truncate_path(path, valid_bytes)?;
        }
        Ok(Self {
            id,
            wal: Wal::open(path)?,
            current_term: state.current_term,
            voted_for: state.voted_for,
            log: state.log,
            commit_index: 0,
            last_applied: 0,
            role: Role::Follower,
            leader_id: None,
            election_timeout_ms: 150 + mix(seed ^ id) % 151,
        })
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn role(&self) -> Role {
        self.role
    }

    pub fn term(&self) -> u64 {
        self.current_term
    }

    pub fn voted_for(&self) -> Option<u64> {
        self.voted_for
    }

    pub fn log(&self) -> &[Entry] {
        &self.log
    }

    pub fn commit_index(&self) -> u64 {
        self.commit_index
    }

    pub fn election_timeout_ms(&self) -> u64 {
        self.election_timeout_ms
    }

    pub fn last_log_index(&self) -> u64 {
        self.log.len() as u64
    }

    pub fn last_log_term(&self) -> u64 {
        self.log.last().map(|entry| entry.term).unwrap_or(0)
    }

    pub fn start_election(&mut self) -> io::Result<RequestVote> {
        self.current_term = self
            .current_term
            .checked_add(1)
            .ok_or_else(|| invalid("raft: term exhausted"))?;
        self.role = Role::Candidate;
        self.leader_id = None;
        self.voted_for = Some(self.id);
        self.persist()?;
        Ok(RequestVote {
            term: self.current_term,
            candidate_id: self.id,
            last_log_index: self.last_log_index(),
            last_log_term: self.last_log_term(),
        })
    }

    pub fn become_leader(&mut self) {
        self.role = Role::Leader;
        self.leader_id = Some(self.id);
    }

    pub fn observe_term(&mut self, term: u64) -> io::Result<()> {
        if term > self.current_term {
            self.current_term = term;
            self.voted_for = None;
            self.role = Role::Follower;
            self.leader_id = None;
            self.persist()?;
        }
        Ok(())
    }

    pub fn request_vote(&mut self, request: &RequestVote) -> io::Result<VoteResponse> {
        if request.term < self.current_term {
            return Ok(VoteResponse {
                term: self.current_term,
                granted: false,
            });
        }
        let mut changed = false;
        if request.term > self.current_term {
            self.current_term = request.term;
            self.voted_for = None;
            self.role = Role::Follower;
            self.leader_id = None;
            changed = true;
        }
        let up_to_date = (request.last_log_term, request.last_log_index)
            >= (self.last_log_term(), self.last_log_index());
        let can_vote = self.voted_for.is_none() || self.voted_for == Some(request.candidate_id);
        let granted = can_vote && up_to_date;
        if granted && self.voted_for != Some(request.candidate_id) {
            self.voted_for = Some(request.candidate_id);
            changed = true;
        }
        if changed {
            self.persist()?;
        }
        Ok(VoteResponse {
            term: self.current_term,
            granted,
        })
    }

    pub fn append_entries(&mut self, request: &AppendEntries) -> io::Result<AppendResponse> {
        if request.term < self.current_term {
            return Ok(AppendResponse {
                term: self.current_term,
                success: false,
                match_index: self.last_log_index(),
            });
        }

        let mut persistent_changed = false;
        if request.term > self.current_term {
            self.current_term = request.term;
            self.voted_for = None;
            persistent_changed = true;
        }
        self.role = Role::Follower;
        self.leader_id = Some(request.leader_id);

        if request.prev_log_index > self.last_log_index()
            || (request.prev_log_index > 0
                && self.log[(request.prev_log_index - 1) as usize].term
                    != request.prev_log_term)
        {
            if persistent_changed {
                self.persist()?;
            }
            return Ok(AppendResponse {
                term: self.current_term,
                success: false,
                match_index: self.last_log_index(),
            });
        }

        let mut index = request.prev_log_index as usize;
        for incoming in &request.entries {
            if index < self.log.len() {
                if self.log[index].term != incoming.term
                    || self.log[index].command != incoming.command
                {
                    self.log.truncate(index);
                    self.log.push(incoming.clone());
                    persistent_changed = true;
                }
            } else {
                self.log.push(incoming.clone());
                persistent_changed = true;
            }
            index += 1;
        }
        if self.log.len() > index {
            self.log.truncate(index);
            persistent_changed = true;
        }
        if persistent_changed {
            self.persist()?;
        }

        if request.leader_commit > self.commit_index {
            self.commit_index = request.leader_commit.min(self.last_log_index());
        }
        Ok(AppendResponse {
            term: self.current_term,
            success: true,
            match_index: request.prev_log_index + request.entries.len() as u64,
        })
    }

    pub fn append_as_leader(&mut self, command: Vec<u8>) -> io::Result<u64> {
        if self.role != Role::Leader {
            return Err(io::Error::new(io::ErrorKind::Other, "raft: not leader"));
        }
        self.log.push(Entry {
            term: self.current_term,
            command,
        });
        self.persist()?;
        Ok(self.last_log_index())
    }

    pub fn advance_commit(&mut self, replicated: &[u64]) -> u64 {
        if self.role != Role::Leader {
            return self.commit_index;
        }
        for candidate in (self.commit_index + 1..=self.last_log_index()).rev() {
            let copies = 1 + replicated
                .iter()
                .filter(|index| **index >= candidate)
                .count();
            if copies * 2 > replicated.len() + 1
                && self.log[(candidate - 1) as usize].term == self.current_term
            {
                self.commit_index = candidate;
                break;
            }
        }
        self.commit_index
    }

    pub fn take_applied(&mut self) -> Vec<Entry> {
        let mut applied = Vec::new();
        while self.last_applied < self.commit_index {
            applied.push(self.log[self.last_applied as usize].clone());
            self.last_applied += 1;
        }
        applied
    }

    fn persist(&mut self) -> io::Result<()> {
        let state = PersistentState {
            current_term: self.current_term,
            voted_for: self.voted_for,
            log: self.log.clone(),
        };
        self.wal.append(&state.encode()?)?;
        self.wal.sync()
    }
}

#[derive(Default)]
struct PersistentState {
    current_term: u64,
    voted_for: Option<u64>,
    log: Vec<Entry>,
}

impl PersistentState {
    fn encode(&self) -> io::Result<Vec<u8>> {
        let count = u32::try_from(self.log.len())
            .map_err(|_| invalid("raft: log too large to encode"))?;
        let mut out = vec![STATE_TAG, STATE_VERSION];
        put_u64(&mut out, self.current_term);
        match self.voted_for {
            Some(node) => {
                out.push(1);
                put_u64(&mut out, node);
            }
            None => {
                out.push(0);
                put_u64(&mut out, 0);
            }
        }
        out.extend_from_slice(&count.to_le_bytes());
        for entry in &self.log {
            let len = u32::try_from(entry.command.len())
                .map_err(|_| invalid("raft: command too large"))?;
            put_u64(&mut out, entry.term);
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&entry.command);
        }
        Ok(out)
    }

    fn decode(bytes: &[u8]) -> io::Result<Self> {
        let mut cursor = Cursor { bytes, offset: 0 };
        if cursor.u8()? != STATE_TAG || cursor.u8()? != STATE_VERSION {
            return Err(invalid("raft: bad persistent state header"));
        }
        let current_term = cursor.u64()?;
        let has_vote = cursor.u8()?;
        let vote = cursor.u64()?;
        let voted_for = match has_vote {
            0 => None,
            1 => Some(vote),
            _ => return Err(invalid("raft: invalid vote flag")),
        };
        let count = cursor.u32()? as usize;
        let mut log = Vec::with_capacity(count);
        for _ in 0..count {
            let term = cursor.u64()?;
            let len = cursor.u32()? as usize;
            let command = cursor.bytes(len)?.to_vec();
            log.push(Entry { term, command });
        }
        if cursor.offset != bytes.len() {
            return Err(invalid("raft: trailing persistent state bytes"));
        }
        Ok(Self {
            current_term,
            voted_for,
            log,
        })
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn u8(&mut self) -> io::Result<u8> {
        let offset = self.take(1)?;
        Ok(self.bytes[offset])
    }

    fn u32(&mut self) -> io::Result<u32> {
        let offset = self.take(4)?;
        Ok(u32::from_le_bytes(
            self.bytes[offset..offset + 4]
                .try_into()
                .expect("fixed slice"),
        ))
    }

    fn u64(&mut self) -> io::Result<u64> {
        let offset = self.take(8)?;
        Ok(u64::from_le_bytes(
            self.bytes[offset..offset + 8]
                .try_into()
                .expect("fixed slice"),
        ))
    }

    fn bytes(&mut self, len: usize) -> io::Result<&'a [u8]> {
        let offset = self.take(len)?;
        Ok(&self.bytes[offset..offset + len])
    }

    fn take(&mut self, len: usize) -> io::Result<usize> {
        let start = self.offset;
        self.offset = self
            .offset
            .checked_add(len)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| invalid("raft: state length exceeds record"))?;
        Ok(start)
    }
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn mix(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
