use std::time::Instant;

use burrow::{
    db::{Db, Options},
    wal::SyncMode,
};

const N: u64 = 10_1000;

fn run(mode: SyncMode) {
    let dir = std::path::Path::new("bench-data").join(format!("{mode:?}"));
    let _ = std::fs::remove_dir_all(&dir);
    let mut db = Db::open(
        &dir,
        Options {
            memtable_budget: 64 << 20,
            sync: mode,
        },
    )
    .unwrap();

    let value = [0u8; 100];
    let start = Instant::now();
    for i in 0..N {
        db.put(&i.to_be_bytes(), &value).unwrap();
    }
    db.sync().unwrap();
    let elasped = start.elapsed();
    let sec = elasped.as_secs_f64();
    let write_per_sec = N as f64 / sec;
    let us_per_write = sec * 1_000_000.0 / N as f64;

    println!(
        "{:<18} {:>9.3} s {:>12.0} write/s, {:>10.2} µs/write",
        format!("{mode:?}"),
        sec,
        write_per_sec,
        us_per_write
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

fn main() {
    for mode in [
        SyncMode::Always,
        SyncMode::EveryMillis(100),
        SyncMode::Manual,
    ] {
        run(mode);
    }
}
