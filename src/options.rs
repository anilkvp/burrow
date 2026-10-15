//! [TODO:description]

pub struct Options {
    pub memtable_budget: usize,    // e.g 4MiB
    pub block_size: usize,         // e.g. 4MiB
    pub block_cache_bytes: usize,  // e.g. 8MiB
    pub bloom_bits_per_key: usize, // 10 (about 1% false positive)
    pub max_open_files: usize,     // e.g. 64
}
