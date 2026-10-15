// write ahead log - write memtable content into a file.

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
pub enum Decode {
    Record(Record, usize),
    Eof,
    Truncated,
    Corrupt(&'static str),
}

fn read_u32(buf: &[u8], at: usize) -> usize {
    u32::from_le_bytes(buf[at..at + 4].try_into().unwrap()) as usize
}

pub fn decode(buf: &[u8]) -> Decode {
    if buf.is_empty() {
        return Decode::Eof;
    }
    if buf.len() < HEADER_LEN {
        return Decode::Truncated;
    }

    let payload_len = read_u32(buf, 4);
    if payload_len > MAX_PAYLOAD {
        return Decode::Corrupt("payload length is too large");
    }
    if payload_len < MIN_PAYLOAD {
        return Decode::Corrupt("payload length is too small");
    }

    let total = HEADER_LEN + payload_len;
    if buf.len() < total {
        return Decode::Truncated;
    }
    if read_u32(buf, 0) as u32 != crc32fast::hash(&buf[4..total]) {
        return Decode::Corrupt("checksum mismatch");
    }

    let p = &buf[HEADER_LEN..total];
    let key_len = read_u32(p, 1);

    let key_end = 5 + key_len;
    if key_end + 5 > p.len() {
        return Decode::Corrupt("key length out of bound");
    }

    let key = &p[5..key_end];

    let val_len = read_u32(p, key_end);

    if key_end + 4 + val_len != p.len() {
        return Decode::Corrupt("value length mismatch");
    }

    let value = &p[key_end + 4..];

    let rec = match p[0] {
        1 => Record::Put {
            key: key.to_vec(),
            value: value.to_vec(),
        },
        2 if value.is_empty() => Record::Delete { key: key.to_vec() },
        2 => return Decode::Corrupt("delete with  with  value"),
        _ => return Decode::Corrupt("unknown value fro record"),
    };
    Decode::Record(rec, total)
}
