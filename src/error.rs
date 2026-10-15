use crate::memtable::MemTableError;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("io error {0}")]
    Io(#[from] std::io::Error),

    #[error("Corruption at bytes {offset}: {reason}")]
    Corruption { offset: u64, reason: &'static str },

    #[error(transparent)]
    MemTable(#[from] MemTableError),
}

pub type Result<T> = std::result::Result<T, Error>;
