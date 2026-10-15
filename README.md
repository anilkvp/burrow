# Burrow

![Status](https://img.shields.io/badge/status-in%20development-yellow)

A small LSM-tree key-value store in Rust that never uses more memory than you give it.

Most storage engines treat memory as something to tune. Burrow treats it as a hard limit: every byte held in RAM comes from an explicit budget, every write is checked against that budget before anything changes, and every write that returns Ok survives a crash.

It's built from first principles, with almost no dependencies, as a way to learn and demonstrate how real databases like RocksDB, LevelDB and Cassandra store data underneath.

> **Status:** early development. The memtable and write-ahead log work and are
> tested; on-disk SSTables, compaction and caching are in progress. See the
> [roadmap](#roadmap).

---

### How it works

Burrow is a log-structured merge tree (LSM-tree). The one rule behind it: never modify data on disk in place; only append, and clean up later in large batches. Disks, and SSDs in particular, are far faster at sequential writes than random ones.

[TODO Diagram]

Writes go to the log first, then to the memtable, and only then return Ok. Reads check the newest data first: memtable, then (planned) SSTables from newest to oldest. The first answer wins, and a tombstone means "deleted, stop looking".

### Key terms

| Term       | Meaning                                                               |
| ---------- | --------------------------------------------------------------------- |
| Memtable   | Sorted in-memory buffer for recent writes, capped by a byte budget    |
| Tombstone  | Marker recording a delete, so older copies on disk stay hidden        |
| WAL        | Write-ahead log: every change is appended here before it takes effect |
| fsync      | Asking the OS to wait until data is physically on disk                |
| SSTable    | Immutable, sorted on-disk file of key-value pairs _(planned)_         |
| Compaction | Merging SSTables to drop overwritten and deleted data _(planned)_     |

---

## Durability

Burrow makes one promise: **if a write returned `Ok`, it survives a crash.**
How strongly that holds against power loss is a choice, because `fsync` is slow:

| `SyncMode`       | Speed   | What a power cut can lose              |
| ---------------- | ------- | -------------------------------------- |
| `Always`         | Slowest | Nothing                                |
| `EveryMillis(n)` | Fast    | Up to the last `n` ms of writes        |
| `Manual`         | Fastest | Everything since your last `db.sync()` |

### WAL (Write-Ahead Log) record format

```
| crc32 (4) | payload_len (4) | type (1) | key_len (4) | key | val_len (4) | value |
            └────────────────────── payload ─────────────────────────────────────┘
```

Type byte:
1 = Put, 2 = Delete. A delete has val_len = 0 and no value. Reject any other type byte as corruption.
