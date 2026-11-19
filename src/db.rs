use std::path::{Path, PathBuf};

use crate::{
    error::Result,
    memtable::{Lookup, MemTable},
    wal::{self, Record, SyncMode, WalWriter},
};

#[derive(Debug, Clone)]
pub struct Options {
    pub memtable_budget: usize,
    pub sync: SyncMode,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            memtable_budget: 4 * 1024 * 1024,
            sync: SyncMode::Always,
        }
    }
}

const WAL_NAME: &str = "000001.wal";
pub struct Db {
    dir: PathBuf,
    wal: WalWriter,
    mem: MemTable,
}

impl Db {
    pub fn open(dir: impl AsRef<Path>, opts: Options) -> Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir)?;

        let wal_path = dir.join(WAL_NAME);
        let is_new = !wal_path.exists();

        // 1. Read back everything logged.
        let rep = wal::replay(&wal_path)?;

        // 2. Cut a torn  tail  BEFORE  anything new  is appened.
        if rep.file_len > rep.valid_len {
            wal::truncate_to(&wal_path, rep.valid_len)?;
        }

        // 3. Rebuild memtable
        let mut mem = MemTable::new(opts.memtable_budget);
        for rec in rep.records {
            match rec {
                Record::Put { key, value } => mem.put(key, value)?,
                Record::Delete { key } => mem.delete(key)?,
            }
        }
        // 4. Only now open  the log for writing.
        let wal = WalWriter::open(&wal_path, opts.sync)?;
        if is_new {
            wal.sync_dir(&dir)?;
        }
        Ok(Self { dir, wal, mem })
    }

    pub fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        self.mem.check_fit(key, value.len())?;
        self.wal.append(wal::Kind::Put, key, value)?;
        self.mem.put(key.to_vec(), value.to_vec())?;
        Ok(())
    }

    pub fn delete(&mut self, key: &[u8]) -> Result<()> {
        self.wal.append(wal::Kind::Delete, key, &[])?;
        self.mem.delete(key.to_vec())?;
        Ok(())
    }

    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let k = self.mem.get(key);
        match k {
            Lookup::Found(v) => Ok(Some(v.to_vec())),
            Lookup::Deleted | Lookup::NotFound => Ok(None),
        }
    }

    pub fn sync(&mut self) -> Result<()> {
        self.wal.sync()
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use crate::{
        db::{Db, Options, WAL_NAME},
        wal,
    };

    #[test]
    fn test_torn_tail_is_cut_so_new_write_survive() {
        let dir = tempfile::tempdir().unwrap();
        let opts = || Options {
            memtable_budget: 1 << 20,
            sync: crate::wal::SyncMode::Always,
        };

        {
            let mut db = Db::open(dir.path(), opts()).unwrap();
            for k in [b"k1", b"k2", b"k3"] {
                db.put(k, b"v").unwrap();
            }
        }

        let mut half = Vec::new();
        wal::encode(wal::Kind::Put, b"torn", b"value", &mut half);
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(dir.path().join(WAL_NAME))
            .unwrap();
        f.write_all(&half[..&half.len() / 2]).unwrap();
        drop(f);

        {
            let mut db = Db::open(dir.path(), opts()).unwrap();
            assert_eq!(db.get(b"torn").unwrap(), None);
            db.put(b"k4", b"v").unwrap();
            db.put(b"k5", b"v").unwrap();
        }

        let db = Db::open(dir.path(), opts()).unwrap();
        for k in [b"k1", b"k2", b"k3", b"k4", b"k5"] {
            assert!(db.get(k).unwrap().is_some());
        }
    }
}
