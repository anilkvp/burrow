use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
};

use burrow::db::{Db, Options};

const DIR_VAR: &str = "BURROW_CRASH_DIR";

fn key(i: u64) -> Vec<u8> {
    format!("key{i:08}").into_bytes()
}

fn value(i: u64) -> Vec<u8> {
    format!("value-{i}").into_bytes()
}

fn parse(s: &str) -> Option<u64> {
    let k = s.strip_prefix("acked ")?;
    k.parse().ok()
}

#[test]
fn crash_child() {
    let Ok(dir) = std::env::var(DIR_VAR) else {
        return;
    };
    let opts = Options {
        memtable_budget: 64 << 20,
        sync: burrow::wal::SyncMode::Always,
    };
    let mut db = Db::open(&dir, opts).unwrap();
    let mut out = std::io::stdout();

    for i in 0.. {
        db.put(&key(i), &value(i)).unwrap();
        writeln!(out, "acked {i}").unwrap();
        out.flush().unwrap();
    }
}

#[test]
fn acknowledged_writes_survive_kill() {
    for round in 0..20 {
        let dir = tempfile::tempdir().unwrap();
        let kill_after = 5;
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["crash_child", "--exact", "--nocapture"])
            .env(DIR_VAR, dir.path())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let mut last_acked = None;
        for l in lines.by_ref() {
            let Ok(line) = l else { break };
            let Some(i) = parse(&line) else { continue };
            last_acked = Some(i);

            if i >= kill_after {
                break;
            }
        }

        child.kill().unwrap();
        child.wait().unwrap();

        for line in lines.map_while(|l| l.ok()) {
            if let Some(i) = parse(&line) {
                last_acked = Some(i);
            }
        }
        let last = last_acked.expect("child acked nothing");
        eprintln!("round {round}: kill_after {kill_after}, last_acked {last}");
        let db = Db::open(
            dir.path(),
            Options {
                memtable_budget: 64 << 20,
                sync: burrow::wal::SyncMode::Always,
            },
        )
        .unwrap();
        for i in 0..=last {
            assert_eq!(db.get(&key(i)).unwrap(), Some(value(i)), "key {i} lost");
        }

        assert_eq!(db.get(&key(last + 2)).unwrap(), None, "invented data");
    }
}
