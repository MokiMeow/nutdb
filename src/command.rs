//! The mutations written to the log.
//!
//! Encoding (little-endian):
//!
//! ```text
//! Set:    [0x01][klen: u32][key][vlen: u32][value]
//! Delete:     [0x02][klen: u32][key]
//! Checkpoint: [0x03]
//! ```
//!
//! Deliberately explicit rather than using a serialisation crate — the on-disk
//! format is part of the database's contract, so it is written by hand and
//! documented here.

use std::io;

const TAG_SET: u8 = 0x01;
const TAG_DELETE: u8 = 0x02;
const TAG_CHECKPOINT: u8 = 0x03;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Set { key: String, value: String },
    Delete { key: String },
    Checkpoint,
}

impl Command {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        match self {
            Command::Set { key, value } => {
                out.push(TAG_SET);
                push_bytes(&mut out, key.as_bytes());
                push_bytes(&mut out, value.as_bytes());
            }
            Command::Delete { key } => {
                out.push(TAG_DELETE);
                push_bytes(&mut out, key.as_bytes());
            }
            Command::Checkpoint => out.push(TAG_CHECKPOINT),
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        let mut cursor = Cursor { bytes, offset: 0 };
        let tag = cursor.take_u8()?;
        let command = match tag {
            TAG_SET => Command::Set {
                key: cursor.take_string()?,
                value: cursor.take_string()?,
            },
            TAG_DELETE => Command::Delete {
                key: cursor.take_string()?,
            },
            TAG_CHECKPOINT => Command::Checkpoint,
            other => {
                return Err(invalid(format!("unknown command tag {other:#04x}")));
            }
        };
        if cursor.offset != bytes.len() {
            return Err(invalid("trailing bytes after command"));
        }
        Ok(command)
    }
}

fn push_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(bytes);
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl Cursor<'_> {
    fn take_u8(&mut self) -> io::Result<u8> {
        let byte = *self
            .bytes
            .get(self.offset)
            .ok_or_else(|| invalid("unexpected end of command"))?;
        self.offset += 1;
        Ok(byte)
    }

    fn take_u32(&mut self) -> io::Result<usize> {
        let end = self.offset + 4;
        let slice = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| invalid("unexpected end of command"))?;
        self.offset = end;
        Ok(u32::from_le_bytes(slice.try_into().unwrap()) as usize)
    }

    fn take_string(&mut self) -> io::Result<String> {
        let len = self.take_u32()?;
        let end = self.offset + len;
        let slice = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| invalid("command length exceeds record"))?;
        self.offset = end;
        String::from_utf8(slice.to_vec()).map_err(|_| invalid("key or value is not valid UTF-8"))
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::Command;

    #[test]
    fn set_round_trips() {
        let command = Command::Set {
            key: "user:1".into(),
            value: "ada".into(),
        };
        assert_eq!(Command::decode(&command.encode()).unwrap(), command);
    }

    #[test]
    fn delete_round_trips() {
        let command = Command::Delete { key: "user:1".into() };
        assert_eq!(Command::decode(&command.encode()).unwrap(), command);
    }

    #[test]
    fn handles_unicode_and_empty_values() {
        for command in [
            Command::Set { key: "ключ".into(), value: "значение".into() },
            Command::Set { key: "k".into(), value: String::new() },
        ] {
            assert_eq!(Command::decode(&command.encode()).unwrap(), command);
        }
    }

    #[test]
    fn rejects_garbage() {
        assert!(Command::decode(&[0xFF, 0, 0, 0, 0]).is_err());
        assert!(Command::decode(&[]).is_err());
        // A length that runs past the end of the record.
        assert!(Command::decode(&[0x01, 0xFF, 0xFF, 0, 0]).is_err());
    }
}
