# 01 — Getting started

## Requirements

A Rust toolchain (1.70+). Nothing else — nutdb has zero dependencies.

```bash
sudo apt-get update && sudo apt-get install -y cargo rustc
# or: curl https://sh.rustup.rs -sSf | sh
```

## Run the durability demo

```bash
cargo run --release -- demo
```

It writes several keys, deletes one, drops the store (which is exactly what a
process crash looks like to the data), reopens from the log alone, and verifies
every committed write came back:

```
nutdb milestone 0 — write-ahead log durability

opening a fresh store at data/demo.wal
  set user:1 = ada
  ...
  delete temp
  3 keys live, log is 126 bytes

process 'crashes' — reopening from the log only
  replayed 5 records (truncated: false)
  keys recovered: ["user:1", "user:2", "user:3"]

durability verified: every committed write survived, the delete stuck
```

## Run the tests

```bash
cargo test
```

10 crash-recovery tests plus unit tests. The interesting ones deliberately
corrupt the log:

- `torn_write_is_discarded_and_earlier_writes_survive`
- `corrupted_payload_fails_its_checksum`
- `truncated_header_is_handled`

## Use the CLI

```bash
cargo run -- set user:1 ada
cargo run -- get user:1        # ada
cargo run -- list
```

Data lives in `data/nutdb.wal`. Delete it to start fresh.

## Inspect the on-disk format

The log is `[len: u32][crc: u32][payload]` records, little-endian:

```bash
hexdump -C data/nutdb.wal | head
```

Reading the bytes directly is the surest way to confirm a format change did what
you intended — see [docs/05](05-durability.md).

## Troubleshooting

- **`truncated: true` on open** — the log ended in a torn or corrupt record.
  That is the recovery path working: committed data before it is intact, and
  `Recovery::valid_bytes` tells you where the good prefix ends.
- **Permission errors** — the store creates `data/` relative to the working
  directory; run from the repo root.
