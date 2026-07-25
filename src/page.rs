//! Fixed-size slotted pages used by the storage engine.
//!
//! The slot array grows forward and cells grow backward:
//!
//! ```text
//! 0       4  5  6       8       10      12      16
//! +-------+--+--+-------+-------+-------+-------+----------+
//! | magic |ver|ty| count | front | back  | crc32 | slots -> |
//! +-------+--+--+-------+-------+-------+-------+----------+
//! |                    free space                    |
//! +--------------------------------------------------+
//! | <- [len:u16][cell] [len:u16][cell] ...          |
//! +--------------------------------------------------+ 4096
//! ```
//!
//! The checksum covers the complete page with the checksum field zeroed.

use std::io;

use crate::crc::crc32;

pub const PAGE_SIZE: usize = 4096;
const HEADER_SIZE: usize = 16;
const MAGIC: &[u8; 4] = b"NPG1";
const VERSION: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PageKind {
    Meta = 1,
    Internal = 2,
    Leaf = 3,
}

impl PageKind {
    fn decode(value: u8) -> io::Result<Self> {
        match value {
            1 => Ok(Self::Meta),
            2 => Ok(Self::Internal),
            3 => Ok(Self::Leaf),
            _ => Err(invalid("page: unknown page kind")),
        }
    }
}

#[derive(Clone)]
pub struct SlottedPage {
    bytes: [u8; PAGE_SIZE],
}

impl SlottedPage {
    pub fn new(kind: PageKind) -> Self {
        let mut bytes = [0u8; PAGE_SIZE];
        bytes[0..4].copy_from_slice(MAGIC);
        bytes[4] = VERSION;
        bytes[5] = kind as u8;
        put_u16(&mut bytes, 6, 0);
        put_u16(&mut bytes, 8, HEADER_SIZE as u16);
        put_u16(&mut bytes, 10, PAGE_SIZE as u16);
        Self { bytes }
    }

    pub fn from_bytes(bytes: [u8; PAGE_SIZE]) -> io::Result<Self> {
        if &bytes[0..4] != MAGIC {
            return Err(invalid("page: bad magic"));
        }
        if bytes[4] != VERSION {
            return Err(invalid("page: unsupported version"));
        }
        PageKind::decode(bytes[5])?;

        let count = get_u16(&bytes, 6) as usize;
        let front = get_u16(&bytes, 8) as usize;
        let back = get_u16(&bytes, 10) as usize;
        if front != HEADER_SIZE + count * 2 || front > back || back > PAGE_SIZE {
            return Err(invalid("page: invalid free-space bounds"));
        }

        let stored = u32::from_le_bytes(bytes[12..16].try_into().expect("fixed slice"));
        let mut checked = bytes;
        checked[12..16].fill(0);
        if crc32(&checked) != stored {
            return Err(invalid("page: checksum mismatch"));
        }

        let page = Self { bytes };
        page.cells()?;
        Ok(page)
    }

    pub fn kind(&self) -> PageKind {
        PageKind::decode(self.bytes[5]).expect("constructed page kind")
    }

    pub fn insert(&mut self, cell: &[u8]) -> io::Result<()> {
        let len = u16::try_from(cell.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "page: cell too large"))?;
        let count = get_u16(&self.bytes, 6) as usize;
        let front = get_u16(&self.bytes, 8) as usize;
        let back = get_u16(&self.bytes, 10) as usize;
        let needed = 2usize + 2 + cell.len();
        if back.saturating_sub(front) < needed {
            return Err(io::Error::new(io::ErrorKind::WriteZero, "page: no free space"));
        }

        let cell_at = back - 2 - cell.len();
        put_u16(&mut self.bytes, cell_at, len);
        self.bytes[cell_at + 2..back].copy_from_slice(cell);
        put_u16(&mut self.bytes, front, cell_at as u16);
        put_u16(&mut self.bytes, 6, (count + 1) as u16);
        put_u16(&mut self.bytes, 8, (front + 2) as u16);
        put_u16(&mut self.bytes, 10, cell_at as u16);
        Ok(())
    }

    pub fn cells(&self) -> io::Result<Vec<&[u8]>> {
        let count = get_u16(&self.bytes, 6) as usize;
        let mut cells = Vec::with_capacity(count);
        for index in 0..count {
            let slot = HEADER_SIZE + index * 2;
            let offset = get_u16(&self.bytes, slot) as usize;
            if offset < HEADER_SIZE || offset + 2 > PAGE_SIZE {
                return Err(invalid("page: slot points outside page"));
            }
            let len = get_u16(&self.bytes, offset) as usize;
            let start = offset + 2;
            let end = start
                .checked_add(len)
                .filter(|end| *end <= PAGE_SIZE)
                .ok_or_else(|| invalid("page: cell extends outside page"))?;
            cells.push(&self.bytes[start..end]);
        }
        Ok(cells)
    }

    pub fn finish(mut self) -> [u8; PAGE_SIZE] {
        self.bytes[12..16].fill(0);
        let checksum = crc32(&self.bytes);
        self.bytes[12..16].copy_from_slice(&checksum.to_le_bytes());
        self.bytes
    }
}

fn get_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().expect("fixed slice"))
}

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::{PageKind, SlottedPage};

    #[test]
    fn cells_round_trip_and_corruption_is_detected() {
        let mut page = SlottedPage::new(PageKind::Leaf);
        page.insert(b"alpha").unwrap();
        page.insert(b"beta").unwrap();
        let bytes = page.finish();
        let decoded = SlottedPage::from_bytes(bytes).unwrap();
        assert_eq!(decoded.kind(), PageKind::Leaf);
        assert_eq!(decoded.cells().unwrap(), [b"alpha".as_slice(), b"beta"]);

        let mut corrupt = bytes;
        corrupt[100] ^= 1;
        assert!(SlottedPage::from_bytes(corrupt).is_err());
    }
}
