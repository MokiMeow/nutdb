//! A bounded register-history linearizability checker.
//!
//! Completed operations are searched in every order allowed by their real-time
//! intervals. Failed operations have no effect. Indeterminate writes may be
//! omitted or included, because a timeout does not reveal whether they
//! committed.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Operation {
    Write(String),
    Read,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Ok(Option<String>),
    Fail,
    Info,
}

#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub id: u64,
    pub invoke: u64,
    pub complete: u64,
    pub operation: Operation,
    pub outcome: Outcome,
}

pub fn is_linearizable(history: &[HistoryEntry]) -> bool {
    let mut used = vec![false; history.len()];
    search(history, &mut used, None)
}

fn search(history: &[HistoryEntry], used: &mut [bool], state: Option<String>) -> bool {
    if used.iter().all(|used| *used) {
        return true;
    }
    for index in 0..history.len() {
        if used[index] || has_unordered_predecessor(history, used, index) {
            continue;
        }
        let entry = &history[index];
        used[index] = true;
        let succeeds = match (&entry.operation, &entry.outcome) {
            (_, Outcome::Fail) => search(history, used, state.clone()),
            (Operation::Write(value), Outcome::Ok(_)) => {
                search(history, used, Some(value.clone()))
            }
            (Operation::Read, Outcome::Ok(observed)) if observed == &state => {
                search(history, used, state.clone())
            }
            (Operation::Read, Outcome::Ok(_)) => false,
            (Operation::Write(value), Outcome::Info) => {
                search(history, used, state.clone())
                    || search(history, used, Some(value.clone()))
            }
            (Operation::Read, Outcome::Info) => search(history, used, state.clone()),
        };
        used[index] = false;
        if succeeds {
            return true;
        }
    }
    false
}

fn has_unordered_predecessor(
    history: &[HistoryEntry],
    used: &[bool],
    candidate: usize,
) -> bool {
    history.iter().enumerate().any(|(index, other)| {
        !used[index]
            && index != candidate
            && other.complete < history[candidate].invoke
    })
}
