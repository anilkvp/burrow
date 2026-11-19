# Burrow design notes

## Goals

- Bounded memory: every byte in RAM comes from an explicit budget.
- Durability: a write that returned Ok survives a crash.

## Memtable (M1)

- Cost per entry: key + value + ENTRY_OVERHEAD (32). → why a fixed estimate is fine
- Overwrites charge only the change in value size.
- `freeze()` takes `self` by value. → why a type instead of an `is_frozen` flag

## WAL record format (M2)

| crc32 (4) | payload_len (4) | kind (1) | key_len (4) | key | val_len (4) | value |

- Little-endian everywhere.
- CRC covers everything after itself.
- Max payload 16 MiB, checked before allocating.
- kind: 1 = put, 2 = delete; a delete has no value.

## Write path

check_fit → WAL append → memtable insert → Ok
→ why this order

## Recovery

replay until first bad record → truncate there → rebuild memtable → open writer
→ why stop at the first bad record rather than skipping it
→ why truncate before opening the writer

## Sync modes: measured

| Mode               | µs per write | Writes/sec |    vs Manual |
| ------------------ | -----------: | ---------: | -----------: |
| `Always`           |      1093.63 |        914 | ~270× slower |
| `EveryMillis(100)` |         6.12 |    163,384 | ~1.5× slower |
| `Manual`           |         4.00 |    250,042 |     baseline |

**Conditions:** 101,000 puts per mode, 8-byte unique keys, 100-byte values,
release build, single run. ThinkPad X1 Carbon Gen 11, KXG8AZNV1T02 LA KIOXIA,
filesystem btrfs.

**Takeaways:**

- One fsync costs about 1.1 ms on this disk, which caps `Always` at ~900 writes/sec.
- `EveryMillis(100)` is ~180× faster than `Always`, at the cost of up to
  100 ms of writes in a power cut.
- `Manual`'s ~4 µs is the floor: one `write` syscall per record, because
  `append` flushes every write so a process crash can't lose acknowledged data.## Testing and its limits

- Property tests, truncation and bit-flip tests, crash test.
- → what the crash test proves, and what it does not

## Known limitations

- EveryMillis only syncs when a write arrives.
- One fixed WAL file (numbering comes in M3).
- A full memtable returns Full (flushing comes in M3).

## Self-check answers

1. Why does the CRC cover the length field too? →
2. Why must replay truncate the file before new writes? →
3. What can EveryMillis(100) lose, and in which kind of crash? →
4. Why does put check the memtable before writing the log? →
5. Why doesn't a kill-the-process test prove fsync is correct? →
