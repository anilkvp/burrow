// write ahead log - write memtable content into a file.

use std::{
    fs::{File, OpenOptions},
    io::{BufWriter, Write},
    path::Path,
    time::{Duration, Instant},
};

use crate::error::Result;

const HEADER_LEN: usize = 8;
const MIN_PAYLOAD: usize = 1 + 4 + 4;

pub const MAX_PAYLOAD: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    Put = 1,
    Delete = 2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Record {
    Put { key: Vec<u8>, value: Vec<u8> },
    Delete { key: Vec<u8> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode {
    Always,
    EveryMillis(u64),
    Manual,
}

pub struct WalWriter {
    file: BufWriter<File>,
    buf: Vec<u8>,
    mode: SyncMode,
    last_sync: Instant,
}

impl WalWriter {
    pub fn open(path: &Path, mode: SyncMode) -> Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self {
            file: BufWriter::new(file),
            buf: Vec::with_capacity(4096),
            mode,
            last_sync: Instant::now(),
        })
    }

    pub fn append(&mut self, kind: Kind, key: &[u8], value: &[u8]) -> Result<()> {
        self.buf.clear();
        encode(kind, key, value, &mut self.buf);
        self.file.write_all(&self.buf)?;

        match self.mode {
            SyncMode::Always => self.sync()?,
            SyncMode::EveryMillis(ms) => {
                self.file.flush()?;
                if self.last_sync.elapsed() >= Duration::from_millis(ms) {
                    self.sync()?;
                }
            }
            SyncMode::Manual => self.file.flush()?,
        }
        Ok(())
    }

    pub fn sync(&mut self) -> Result<()> {
        self.file.flush()?;
        self.file.get_ref().sync_data()?;
        self.last_sync = Instant::now();
        Ok(())
    }
}

/// Encode key value pairs into WAL record format.
///
/// * `kind`: Kind
/// * `key`: key
/// * `value`: value
/// * `out`: encoded record
pub fn encode(kind: Kind, key: &[u8], value: &[u8], out: &mut Vec<u8>) {
    let start = out.len();
    let payload_len = MIN_PAYLOAD + key.len() + value.len();

    out.extend_from_slice(&[0u8; 4]);
    out.extend_from_slice(&(payload_len as u32).to_le_bytes());
    out.push(kind as u8);
    out.extend_from_slice(&(key.len() as u32).to_le_bytes());
    out.extend_from_slice(key);
    out.extend_from_slice(&(value.len() as u32).to_le_bytes());
    out.extend_from_slice(value);

    let crc = crc32fast::hash(&out[start + 4..]);
    out[start..start + 4].copy_from_slice(&crc.to_le_bytes());
}

#[derive(Debug, PartialEq, Eq)]
pub enum Decoded {
    Record(Record, usize),
    Eof,
    Truncated,
    Corrupt(&'static str),
}

fn read_u32(buf: &[u8], at: usize) -> usize {
    u32::from_le_bytes(buf[at..at + 4].try_into().unwrap()) as usize
}

/// [TODO:description]
///
/// * `buf`: [TODO:parameter]
pub fn decode(buf: &[u8]) -> Decoded {
    if buf.is_empty() {
        return Decoded::Eof;
    }
    if buf.len() < HEADER_LEN {
        return Decoded::Truncated;
    }

    let payload_len = read_u32(buf, 4);
    if payload_len > MAX_PAYLOAD {
        return Decoded::Corrupt("payload length is too large");
    }
    if payload_len < MIN_PAYLOAD {
        return Decoded::Corrupt("payload length is too small");
    }

    let total = HEADER_LEN + payload_len;
    if buf.len() < total {
        return Decoded::Truncated;
    }
    if read_u32(buf, 0) as u32 != crc32fast::hash(&buf[4..total]) {
        return Decoded::Corrupt("checksum mismatch");
    }

    let p = &buf[HEADER_LEN..total];
    let key_len = read_u32(p, 1);

    let key_end = 5 + key_len;
    if key_end + 5 > p.len() {
        return Decoded::Corrupt("key length out of bound");
    }

    let key = &p[5..key_end];

    let val_len = read_u32(p, key_end);

    if key_end + 4 + val_len != p.len() {
        return Decoded::Corrupt("value length mismatch");
    }

    let value = &p[key_end + 4..];

    let rec = match p[0] {
        1 => Record::Put {
            key: key.to_vec(),
            value: value.to_vec(),
        },
        2 if value.is_empty() => Record::Delete { key: key.to_vec() },
        2 => return Decoded::Corrupt("delete with  with  value"),
        _ => return Decoded::Corrupt("unknown value fro record"),
    };
    Decoded::Record(rec, total)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enc(kind: Kind, k: &[u8], v: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        encode(kind, k, v, &mut out);
        out
    }

    #[test]
    fn exact_size() {
        assert_eq!(enc(Kind::Put, b"ab", b"xyz").len(), 22);
    }

    #[test]
    fn every_cut_is_truncated() {
        let bytes = enc(Kind::Put, b"apple", b"red and shiny");
        for n in 1..bytes.len() {
            assert_eq!(decode(&bytes[..n]), Decoded::Truncated, "cut at {n}");
        }
    }

    #[test]
    fn every_bit_flip_is_detected() {
        let bytes = enc(Kind::Put, b"apple", b"red");
        for i in 0..bytes.len() {
            for bit in 0..8 {
                let mut bad = bytes.clone();
                bad[i] ^= 1 << bit; // flip exactly one bit
                assert!(
                    !matches!(decode(&bad), Decoded::Record(..)),
                    "flip byte {i} bit {bit} was accepted"
                );
            }
        }
    }
}
