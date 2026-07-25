//! Fixed-size page I/O with a bounded least-recently-used cache.
//!
//! Dirty pages are written before eviction. [`Pager::flush`] writes every
//! dirty page and calls `sync_all`, which is the durability boundary used by a
//! checkpoint.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::page::PAGE_SIZE;

pub type PageId = u64;

struct CachedPage {
    bytes: [u8; PAGE_SIZE],
    dirty: bool,
    touched: u64,
}

pub struct Pager {
    file: File,
    path: PathBuf,
    capacity: usize,
    clock: u64,
    pages: u64,
    cache: HashMap<PageId, CachedPage>,
}

impl Pager {
    pub fn open(path: impl AsRef<Path>, capacity: usize) -> io::Result<Self> {
        Self::open_inner(path.as_ref(), capacity, false)
    }

    pub fn create(path: impl AsRef<Path>, capacity: usize) -> io::Result<Self> {
        Self::open_inner(path.as_ref(), capacity, true)
    }

    fn open_inner(path: &Path, capacity: usize, truncate: bool) -> io::Result<Self> {
        if capacity == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "pager: cache capacity must be positive",
            ));
        }
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(truncate)
            .open(path)?;
        let len = file.metadata()?.len();
        if len % PAGE_SIZE as u64 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "pager: file ends in a partial page",
            ));
        }
        Ok(Self {
            file,
            path: path.to_path_buf(),
            capacity,
            clock: 0,
            pages: len / PAGE_SIZE as u64,
            cache: HashMap::new(),
        })
    }

    pub fn page_count(&self) -> u64 {
        self.pages
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn allocate(&mut self) -> io::Result<PageId> {
        let id = self.pages;
        self.pages += 1;
        self.write(id, [0u8; PAGE_SIZE])?;
        Ok(id)
    }

    pub fn read(&mut self, id: PageId) -> io::Result<[u8; PAGE_SIZE]> {
        self.ensure_loaded(id)?;
        self.clock += 1;
        let page = self.cache.get_mut(&id).expect("loaded page");
        page.touched = self.clock;
        Ok(page.bytes)
    }

    pub fn write(&mut self, id: PageId, bytes: [u8; PAGE_SIZE]) -> io::Result<()> {
        if id > self.pages {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "pager: page id skips allocation",
            ));
        }
        if id == self.pages {
            self.pages += 1;
        }
        if !self.cache.contains_key(&id) {
            self.evict_if_needed()?;
        }
        self.clock += 1;
        self.cache.insert(
            id,
            CachedPage {
                bytes,
                dirty: true,
                touched: self.clock,
            },
        );
        Ok(())
    }

    pub fn flush(&mut self) -> io::Result<()> {
        let dirty: Vec<PageId> = self
            .cache
            .iter()
            .filter_map(|(id, page)| page.dirty.then_some(*id))
            .collect();
        for id in dirty {
            self.flush_one(id)?;
        }
        self.file.set_len(self.pages * PAGE_SIZE as u64)?;
        self.file.sync_all()
    }

    fn ensure_loaded(&mut self, id: PageId) -> io::Result<()> {
        if id >= self.pages {
            return Err(io::Error::new(io::ErrorKind::NotFound, "pager: page not found"));
        }
        if self.cache.contains_key(&id) {
            return Ok(());
        }
        self.evict_if_needed()?;
        let mut bytes = [0u8; PAGE_SIZE];
        self.file.seek(SeekFrom::Start(id * PAGE_SIZE as u64))?;
        self.file.read_exact(&mut bytes)?;
        self.cache.insert(
            id,
            CachedPage {
                bytes,
                dirty: false,
                touched: self.clock,
            },
        );
        Ok(())
    }

    fn evict_if_needed(&mut self) -> io::Result<()> {
        if self.cache.len() < self.capacity {
            return Ok(());
        }
        let victim = self
            .cache
            .iter()
            .min_by_key(|(_, page)| page.touched)
            .map(|(id, _)| *id)
            .expect("non-empty cache");
        self.flush_one(victim)?;
        self.cache.remove(&victim);
        Ok(())
    }

    fn flush_one(&mut self, id: PageId) -> io::Result<()> {
        let Some(page) = self.cache.get_mut(&id) else {
            return Ok(());
        };
        if page.dirty {
            self.file.seek(SeekFrom::Start(id * PAGE_SIZE as u64))?;
            self.file.write_all(&page.bytes)?;
            page.dirty = false;
        }
        Ok(())
    }
}

impl Drop for Pager {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::Pager;
    use crate::page::PAGE_SIZE;

    #[test]
    fn dirty_eviction_is_persisted() {
        let path = std::env::temp_dir().join(format!("nutdb-pager-{}", std::process::id()));
        let _ = fs::remove_file(&path);
        {
            let mut pager = Pager::create(&path, 1).unwrap();
            let first = pager.allocate().unwrap();
            let mut a = [0u8; PAGE_SIZE];
            a[0] = 7;
            pager.write(first, a).unwrap();
            let second = pager.allocate().unwrap();
            let mut b = [0u8; PAGE_SIZE];
            b[0] = 9;
            pager.write(second, b).unwrap();
            pager.flush().unwrap();
        }
        let mut pager = Pager::open(&path, 1).unwrap();
        assert_eq!(pager.read(0).unwrap()[0], 7);
        assert_eq!(pager.read(1).unwrap()[0], 9);
        let _ = fs::remove_file(path);
    }
}
