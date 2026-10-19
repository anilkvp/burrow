//! [TODO:description]

use std::{collections::BTreeMap, ops::Bound, sync::Arc};

const ENTRY_OVERHEAD: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    Value(Vec<u8>),
    Tombstone,
}

impl Entry {
    fn payload_len(&self) -> usize {
        match self {
            Entry::Value(vs) => vs.len(),
            Entry::Tombstone => 0,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Lookup<'a> {
    Found(&'a [u8]),
    Deleted,
    NotFound,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum MemTableError {
    #[error("memtable is full: neeed {needed} bytes,  {available} available")]
    Full { needed: usize, available: usize },
    #[error("entry of {size} bytes exceeds memtable budget of {budget}")]
    EntryTooLarge { size: usize, budget: usize },
}

pub struct MemTable {
    map: BTreeMap<Vec<u8>, Entry>,
    size: usize,
    budget: usize,
}

impl MemTable {
    pub fn new(budget: usize) -> Self {
        Self {
            map: BTreeMap::new(),
            size: 0,
            budget,
        }
    }

    pub fn freeze(self) -> FrozenMemTable {
        FrozenMemTable {
            inner: Arc::new(FrozenInner {
                map: self.map,
                size: self.size,
            }),
        }
    }

    pub fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> Result<(), MemTableError> {
        self.insert(key, Entry::Value(value))
    }

    pub fn delete(&mut self, key: Vec<u8>) -> Result<(), MemTableError> {
        self.insert(key, Entry::Tombstone)
    }

    pub fn get(&self, key: &[u8]) -> Lookup<'_> {
        match self.map.get(key) {
            Some(Entry::Value(v)) => Lookup::Found(v),
            Some(Entry::Tombstone) => Lookup::Deleted,
            None => Lookup::NotFound,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&[u8], &Entry)> {
        self.map.iter().map(|(k, e)| (k.as_slice(), e))
    }

    pub fn approximate_size(&self) -> usize {
        self.size
    }

    pub fn budget(&self) -> usize {
        self.budget
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn range<'a>(
        &'a self,
        start: &'a [u8],
        end: &'a [u8],
    ) -> impl Iterator<Item = (&'a [u8], &'a Entry)> {
        self.map
            .range::<[u8], _>((Bound::Included(start), Bound::Excluded(end)))
            .map(|(k, e)| (k.as_slice(), e))
    }

    pub fn check_fit(&self, key: &[u8], payload_len: usize) -> Result<(), MemTableError> {
        self.after_size(key, payload_len).map(|_| ())
    }

    fn insert(&mut self, key: Vec<u8>, value: Entry) -> Result<(), MemTableError> {
        let new_size = self.after_size(&key, value.payload_len())?;
        self.map.insert(key, value);
        self.size = new_size;
        Ok(())
    }

    fn after_size(&self, key: &[u8], payload_len: usize) -> Result<usize, MemTableError> {
        let full_cost = key.len() + payload_len + ENTRY_OVERHEAD;
        if full_cost > self.budget {
            return Err(MemTableError::EntryTooLarge {
                size: full_cost,
                budget: self.budget,
            });
        }
        let new_size = match self.map.get(key) {
            Some(old) => self.size - old.payload_len() + payload_len,
            None => self.size + full_cost,
        };

        if new_size > self.budget {
            return Err(MemTableError::Full {
                needed: new_size - self.size,
                available: self.budget - self.size,
            });
        }
        Ok(new_size)
    }
}

#[derive(Clone)]
pub struct FrozenMemTable {
    inner: Arc<FrozenInner>,
}

struct FrozenInner {
    map: BTreeMap<Vec<u8>, Entry>,
    size: usize,
}

impl FrozenMemTable {
    pub fn get(&self, key: &[u8]) -> Lookup<'_> {
        match self.inner.map.get(key) {
            Some(Entry::Value(v)) => Lookup::Found(v),
            Some(Entry::Tombstone) => Lookup::Deleted,
            None => Lookup::NotFound,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&[u8], &Entry)> {
        self.inner.map.iter().map(|(k, e)| (k.as_slice(), e))
    }

    pub fn approximate_size(&self) -> usize {
        self.inner.size
    }

    pub fn len(&self) -> usize {
        self.inner.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.map.is_empty()
    }

    pub fn range<'a>(
        &'a self,
        start: &'a [u8],
        end: &'a [u8],
    ) -> impl Iterator<Item = (&'a [u8], &'a Entry)> {
        self.inner
            .map
            .range::<[u8], _>((Bound::Included(start), Bound::Excluded(end)))
            .map(|(k, e)| (k.as_slice(), e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(s: &str) -> Vec<u8> {
        s.as_bytes().to_vec()
    }

    #[test]
    fn put_then_get() {
        let mut m = MemTable::new(1024);
        m.put(b("apple"), b("red")).unwrap();
        assert_eq!(m.get(b"apple"), Lookup::Found(b"red"));
        assert_eq!(m.get(b"carrot"), Lookup::NotFound);
    }

    #[test]
    fn delete_leaves_a_tombstone() {
        let mut m = MemTable::new(1024);
        m.put(b("apple"), b("red")).unwrap();
        m.delete(b("apple")).unwrap();
        assert_eq!(m.get(b"apple"), Lookup::Deleted);
        assert_eq!(m.len(), 1); // the tombstone still takes a slot
    }

    #[test]
    fn overwrite_adjusts_size_correctly() {
        let mut m = MemTable::new(1024);
        m.put(b("k"), b("short")).unwrap();
        let before = m.approximate_size();
        m.put(b("k"), b("much longer value")).unwrap();
        assert_eq!(
            m.approximate_size(),
            before + "much longer value".len() - "short".len()
        );
        m.delete(b("k")).unwrap();
        assert_eq!(m.approximate_size(), 1 + ENTRY_OVERHEAD);
    }

    #[test]
    fn never_exceeds_budget() {
        let mut m = MemTable::new(200);
        let mut i = 0;
        loop {
            match m.put(format!("key{i}").into_bytes(), vec![0u8; 20]) {
                Ok(()) => i += 1,
                Err(MemTableError::Full { .. }) => break,
                Err(e) => panic!("unexpected error: {e:?}"),
            }
            assert!(m.approximate_size() <= m.budget());
        }
        assert!(i > 0);
    }

    #[test]
    fn rejected_write_changes_nothing() {
        let mut m = MemTable::new(100);
        m.put(b("a"), vec![0; 40]).unwrap();
        let (len, size) = (m.len(), m.approximate_size());
        assert!(m.put(b("b"), vec![0; 40]).is_err());
        assert_eq!((m.len(), m.approximate_size()), (len, size));
        assert_eq!(m.get(b"b"), Lookup::NotFound);
    }

    #[test]
    fn oversized_entry_is_rejected_up_front() {
        let mut m = MemTable::new(64);
        let err = m.put(b("big"), vec![0; 100]).unwrap_err();
        assert!(matches!(err, MemTableError::EntryTooLarge { .. }));
    }

    #[test]
    fn iteration_and_range_are_sorted() {
        let mut m = MemTable::new(4096);
        for k in ["d", "a", "c", "b", "e"] {
            m.put(b(k), b("v")).unwrap();
        }
        let all: Vec<&[u8]> = m.iter().map(|(k, _)| k).collect();
        assert_eq!(all, vec![&b"a"[..], b"b", b"c", b"d", b"e"]);
        let mid: Vec<&[u8]> = m.range(b"b", b"d").map(|(k, _)| k).collect();
        assert_eq!(mid, vec![&b"b"[..], b"c"]);
    }

    #[test]
    fn freeze_keeps_data_and_size() {
        let mut m = MemTable::new(1024);
        m.put(b("a"), b("1")).unwrap();
        m.delete(b("b")).unwrap();
        let size = m.approximate_size();

        let f = m.freeze();
        // `m.put(...)` here would not compile: `m` was moved into `freeze`.
        assert_eq!(f.get(b"a"), Lookup::Found(b"1"));
        assert_eq!(f.get(b"b"), Lookup::Deleted);
        assert_eq!(f.approximate_size(), size);

        let shared = f.clone(); // cheap: both point at the same map
        assert_eq!(shared.len(), 2);
    }
}
