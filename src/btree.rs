//! A small B-tree index with fixed-page persistence.
//!
//! This is a classic minimum-degree B-tree: keys and values may live in any
//! node, full children are split before descent, and range traversal visits
//! keys in sorted order. Deletes rebuild from the remaining ordered entries;
//! that is deliberately simple and correct for the first storage milestone,
//! while inserts exercise real multi-level node splitting.
//!
//! Persistent layout:
//!
//! ```text
//! page 0 (Meta): [root_page:u64][node_count:u64][entry_count:u64]
//! Leaf cell:     [key_len:u32][value_len:u32][key][value]
//! Internal cell: [key_len:u32][value_len:u32][key][value][right_child:u64]
//! first internal cell: [leftmost_child:u64]
//! ```

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::page::{PageKind, SlottedPage};
use crate::pager::Pager;

const MIN_DEGREE: usize = 8;
const MAX_KEYS: usize = MIN_DEGREE * 2 - 1;

#[derive(Clone, Default)]
struct Node {
    keys: Vec<String>,
    values: Vec<String>,
    children: Vec<usize>,
}

impl Node {
    fn leaf(&self) -> bool {
        self.children.is_empty()
    }
}

#[derive(Clone)]
pub struct BTreeIndex {
    nodes: Vec<Node>,
    root: usize,
    len: usize,
}

impl Default for BTreeIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl BTreeIndex {
    pub fn new() -> Self {
        Self {
            nodes: vec![Node::default()],
            root: 0,
            len: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.get_from(self.root, key)
    }

    fn get_from(&self, node_id: usize, key: &str) -> Option<&str> {
        let node = &self.nodes[node_id];
        match node.keys.binary_search_by(|candidate| candidate.as_str().cmp(key)) {
            Ok(index) => Some(node.values[index].as_str()),
            Err(_) if node.leaf() => None,
            Err(index) => self.get_from(node.children[index], key),
        }
    }

    pub fn insert(&mut self, key: String, value: String) {
        if self.replace(self.root, &key, &value) {
            return;
        }
        if self.nodes[self.root].keys.len() == MAX_KEYS {
            let old_root = self.root;
            self.nodes.push(Node {
                children: vec![old_root],
                ..Node::default()
            });
            self.root = self.nodes.len() - 1;
            self.split_child(self.root, 0);
        }
        self.insert_non_full(self.root, key, value);
        self.len += 1;
    }

    fn replace(&mut self, node_id: usize, key: &str, value: &str) -> bool {
        let (found, child) = {
            let node = &self.nodes[node_id];
            match node.keys.binary_search_by(|candidate| candidate.as_str().cmp(key)) {
                Ok(index) => (Some(index), None),
                Err(_) if node.leaf() => (None, None),
                Err(index) => (None, Some(node.children[index])),
            }
        };
        if let Some(index) = found {
            self.nodes[node_id].values[index] = value.to_owned();
            true
        } else if let Some(child) = child {
            self.replace(child, key, value)
        } else {
            false
        }
    }

    fn insert_non_full(&mut self, node_id: usize, key: String, value: String) {
        if self.nodes[node_id].leaf() {
            let index = self.nodes[node_id]
                .keys
                .binary_search(&key)
                .expect_err("duplicates replaced before insert");
            self.nodes[node_id].keys.insert(index, key);
            self.nodes[node_id].values.insert(index, value);
            return;
        }

        let mut child_index = self.nodes[node_id]
            .keys
            .binary_search(&key)
            .expect_err("duplicates replaced before insert");
        let child_id = self.nodes[node_id].children[child_index];
        if self.nodes[child_id].keys.len() == MAX_KEYS {
            self.split_child(node_id, child_index);
            match key.cmp(&self.nodes[node_id].keys[child_index]) {
                std::cmp::Ordering::Greater => child_index += 1,
                std::cmp::Ordering::Equal => unreachable!("duplicate after split"),
                std::cmp::Ordering::Less => {}
            }
        }
        let child_id = self.nodes[node_id].children[child_index];
        self.insert_non_full(child_id, key, value);
    }

    fn split_child(&mut self, parent_id: usize, child_index: usize) {
        let child_id = self.nodes[parent_id].children[child_index];
        let mut left = self.nodes[child_id].clone();

        let right_keys = left.keys.split_off(MIN_DEGREE);
        let median_key = left.keys.pop().expect("full child");
        let right_values = left.values.split_off(MIN_DEGREE);
        let median_value = left.values.pop().expect("full child");
        let right_children = if left.leaf() {
            Vec::new()
        } else {
            left.children.split_off(MIN_DEGREE)
        };

        self.nodes[child_id] = left;
        self.nodes.push(Node {
            keys: right_keys,
            values: right_values,
            children: right_children,
        });
        let right_id = self.nodes.len() - 1;

        let parent = &mut self.nodes[parent_id];
        parent.keys.insert(child_index, median_key);
        parent.values.insert(child_index, median_value);
        parent.children.insert(child_index + 1, right_id);
    }

    pub fn delete(&mut self, key: &str) -> bool {
        if self.get(key).is_none() {
            return false;
        }
        let retained: Vec<(String, String)> = self
            .entries()
            .into_iter()
            .filter(|(candidate, _)| candidate != key)
            .collect();
        *self = Self::new();
        for (key, value) in retained {
            self.insert(key, value);
        }
        true
    }

    pub fn range(&self, start: &str, end: &str) -> Vec<(String, String)> {
        self.entries()
            .into_iter()
            .filter(|(key, _)| key.as_str() >= start && key.as_str() < end)
            .collect()
    }

    pub fn entries(&self) -> Vec<(String, String)> {
        let mut out = Vec::with_capacity(self.len);
        self.collect(self.root, &mut out);
        out
    }

    pub fn keys(&self) -> Vec<&str> {
        let mut out = Vec::with_capacity(self.len);
        self.collect_keys(self.root, &mut out);
        out
    }

    fn collect(&self, node_id: usize, out: &mut Vec<(String, String)>) {
        let node = &self.nodes[node_id];
        for index in 0..node.keys.len() {
            if !node.leaf() {
                self.collect(node.children[index], out);
            }
            out.push((node.keys[index].clone(), node.values[index].clone()));
        }
        if !node.leaf() {
            self.collect(node.children[node.keys.len()], out);
        }
    }

    fn collect_keys<'a>(&'a self, node_id: usize, out: &mut Vec<&'a str>) {
        let node = &self.nodes[node_id];
        for index in 0..node.keys.len() {
            if !node.leaf() {
                self.collect_keys(node.children[index], out);
            }
            out.push(node.keys[index].as_str());
        }
        if !node.leaf() {
            self.collect_keys(node.children[node.keys.len()], out);
        }
    }

    pub fn save(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let path = path.as_ref();
        let temp = with_suffix(path, ".tmp");
        let backup = with_suffix(path, ".bak");
        let _ = fs::remove_file(&temp);

        let mut pager = Pager::create(&temp, 64)?;
        let meta_id = pager.allocate()?;
        debug_assert_eq!(meta_id, 0);
        for _ in &self.nodes {
            pager.allocate()?;
        }

        let mut meta = SlottedPage::new(PageKind::Meta);
        let mut metadata = Vec::with_capacity(24);
        metadata.extend_from_slice(&(self.root as u64 + 1).to_le_bytes());
        metadata.extend_from_slice(&(self.nodes.len() as u64).to_le_bytes());
        metadata.extend_from_slice(&(self.len as u64).to_le_bytes());
        meta.insert(&metadata)?;
        pager.write(0, meta.finish())?;

        for (index, node) in self.nodes.iter().enumerate() {
            let kind = if node.leaf() {
                PageKind::Leaf
            } else {
                PageKind::Internal
            };
            let mut page = SlottedPage::new(kind);
            if !node.leaf() {
                page.insert(&(node.children[0] as u64 + 1).to_le_bytes())?;
            }
            for item in 0..node.keys.len() {
                let mut cell = encode_pair(&node.keys[item], &node.values[item])?;
                if !node.leaf() {
                    cell.extend_from_slice(&(node.children[item + 1] as u64 + 1).to_le_bytes());
                }
                page.insert(&cell)?;
            }
            pager.write(index as u64 + 1, page.finish())?;
        }
        pager.flush()?;
        drop(pager);

        publish_snapshot(path, &temp, &backup)?;
        sync_parent(path)
    }

    pub fn load(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        let backup = with_suffix(path, ".bak");
        if path.exists() {
            match Self::load_exact(path) {
                Ok(tree) => return Ok(tree),
                Err(_) if backup.exists() => return Self::load_exact(&backup),
                Err(error) => return Err(error),
            }
        }
        if backup.exists() {
            return Self::load_exact(&backup);
        }
        Ok(Self::new())
    }

    fn load_exact(path: &Path) -> io::Result<Self> {
        let selected = path.to_path_buf();
        let mut pager = Pager::open(&selected, 64)?;
        if pager.page_count() == 0 {
            return Ok(Self::new());
        }
        let meta = SlottedPage::from_bytes(pager.read(0)?)?;
        if meta.kind() != PageKind::Meta {
            return Err(invalid("btree: page zero is not metadata"));
        }
        let cells = meta.cells()?;
        if cells.len() != 1 || cells[0].len() != 24 {
            return Err(invalid("btree: malformed metadata"));
        }
        let root_page = take_u64(cells[0], 0)?;
        let node_count = take_u64(cells[0], 8)? as usize;
        let len = take_u64(cells[0], 16)? as usize;
        if root_page == 0 || node_count == 0 || pager.page_count() != node_count as u64 + 1 {
            return Err(invalid("btree: inconsistent metadata"));
        }

        let mut nodes = Vec::with_capacity(node_count);
        for page_id in 1..=node_count as u64 {
            let page = SlottedPage::from_bytes(pager.read(page_id)?)?;
            let cells = page.cells()?;
            let mut node = Node::default();
            let mut first = 0;
            if page.kind() == PageKind::Internal {
                let left = cells
                    .first()
                    .ok_or_else(|| invalid("btree: internal page has no child"))?;
                if left.len() != 8 {
                    return Err(invalid("btree: malformed left child"));
                }
                node.children.push(page_to_index(take_u64(left, 0)?, node_count)?);
                first = 1;
            } else if page.kind() != PageKind::Leaf {
                return Err(invalid("btree: unexpected node page kind"));
            }
            for cell in &cells[first..] {
                let (key, value, used) = decode_pair(cell)?;
                node.keys.push(key);
                node.values.push(value);
                if page.kind() == PageKind::Internal {
                    node.children
                        .push(page_to_index(take_u64(cell, used)?, node_count)?);
                    if used + 8 != cell.len() {
                        return Err(invalid("btree: trailing internal cell bytes"));
                    }
                } else if used != cell.len() {
                    return Err(invalid("btree: trailing leaf cell bytes"));
                }
            }
            if !node.leaf() && node.children.len() != node.keys.len() + 1 {
                return Err(invalid("btree: child/key count mismatch"));
            }
            nodes.push(node);
        }

        let root = page_to_index(root_page, node_count)?;
        let tree = Self { nodes, root, len };
        if tree.entries().len() != len {
            return Err(invalid("btree: entry count mismatch"));
        }
        Ok(tree)
    }
}

#[cfg(not(windows))]
fn publish_snapshot(path: &Path, temp: &Path, backup: &Path) -> io::Result<()> {
    let _ = fs::remove_file(backup);
    if path.exists() {
        fs::rename(path, backup)?;
    }
    if let Err(error) = fs::rename(temp, path) {
        if backup.exists() {
            let _ = fs::rename(backup, path);
        }
        return Err(error);
    }
    let _ = fs::remove_file(backup);
    Ok(())
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::File::open(parent)?.sync_all()?;
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn sync_parent(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(windows)]
fn publish_snapshot(path: &Path, temp: &Path, backup: &Path) -> io::Result<()> {
    use std::fs::OpenOptions;

    let _ = fs::remove_file(backup);
    if path.exists() {
        fs::copy(path, backup)?;
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(backup)?
            .sync_all()?;
    }
    fs::copy(temp, path)?;
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?
        .sync_all()?;
    fs::remove_file(temp)?;
    let _ = fs::remove_file(backup);
    Ok(())
}

fn encode_pair(key: &str, value: &str) -> io::Result<Vec<u8>> {
    let key_len = u32::try_from(key.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "btree: key too large"))?;
    let value_len = u32::try_from(value.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "btree: value too large"))?;
    let mut cell = Vec::with_capacity(8 + key.len() + value.len());
    cell.extend_from_slice(&key_len.to_le_bytes());
    cell.extend_from_slice(&value_len.to_le_bytes());
    cell.extend_from_slice(key.as_bytes());
    cell.extend_from_slice(value.as_bytes());
    Ok(cell)
}

fn decode_pair(cell: &[u8]) -> io::Result<(String, String, usize)> {
    if cell.len() < 8 {
        return Err(invalid("btree: short cell"));
    }
    let key_len = u32::from_le_bytes(cell[0..4].try_into().expect("fixed slice")) as usize;
    let value_len = u32::from_le_bytes(cell[4..8].try_into().expect("fixed slice")) as usize;
    let key_end = 8usize
        .checked_add(key_len)
        .ok_or_else(|| invalid("btree: key length overflow"))?;
    let value_end = key_end
        .checked_add(value_len)
        .filter(|end| *end <= cell.len())
        .ok_or_else(|| invalid("btree: value exceeds cell"))?;
    let key = std::str::from_utf8(&cell[8..key_end])
        .map_err(|_| invalid("btree: key is not UTF-8"))?
        .to_owned();
    let value = std::str::from_utf8(&cell[key_end..value_end])
        .map_err(|_| invalid("btree: value is not UTF-8"))?
        .to_owned();
    Ok((key, value, value_end))
}

fn take_u64(bytes: &[u8], offset: usize) -> io::Result<u64> {
    let end = offset
        .checked_add(8)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| invalid("btree: missing u64"))?;
    Ok(u64::from_le_bytes(
        bytes[offset..end].try_into().expect("fixed slice"),
    ))
}

fn page_to_index(page: u64, count: usize) -> io::Result<usize> {
    let index = page
        .checked_sub(1)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|index| *index < count)
        .ok_or_else(|| invalid("btree: child page out of range"))?;
    Ok(index)
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::BTreeIndex;

    #[test]
    fn splits_searches_ranges_deletes_and_reopens() {
        let mut tree = BTreeIndex::new();
        for i in (0..1000).rev() {
            tree.insert(format!("{i:04}"), format!("value-{i}"));
        }
        assert_eq!(tree.len(), 1000);
        assert_eq!(tree.get("0042"), Some("value-42"));
        assert_eq!(tree.range("0098", "0102").len(), 4);
        assert!(tree.delete("0042"));
        assert_eq!(tree.get("0042"), None);

        let path = std::env::temp_dir().join(format!("nutdb-btree-{}", std::process::id()));
        let _ = fs::remove_file(&path);
        tree.save(&path).unwrap();
        let reopened = BTreeIndex::load(&path).unwrap();
        assert_eq!(reopened.entries(), tree.entries());
        let _ = fs::remove_file(path);
    }
}
