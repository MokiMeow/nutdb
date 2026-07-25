//! Raft RPC types. Log indices are one-based; index zero is the sentinel before
//! the first entry.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub term: u64,
    pub command: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct RequestVote {
    pub term: u64,
    pub candidate_id: u64,
    pub last_log_index: u64,
    pub last_log_term: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoteResponse {
    pub term: u64,
    pub granted: bool,
}

#[derive(Clone, Debug)]
pub struct AppendEntries {
    pub term: u64,
    pub leader_id: u64,
    pub prev_log_index: u64,
    pub prev_log_term: u64,
    pub entries: Vec<Entry>,
    pub leader_commit: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppendResponse {
    pub term: u64,
    pub success: bool,
    pub match_index: u64,
}
