# CLAUDE.md

This is a **lightweight fork** of [nimiq/core-rs-albatross](https://github.com/nicmiq/core-rs-albatross), stripped down to a dedicated history node that exposes only 11 RPC methods. It is NOT a general-purpose Nimiq node.

## Build & Run

```bash
cargo build -p nimiq-client            # Debug build
cargo build -p nimiq-client --release  # Release build
```

The binary requires a TOML config file. Key settings:

```toml
sync_mode = "history"

[consensus]
sync_mode = "history"
index_history = true   # Required — enables address-based tx lookups

[database]
path = "./db"

[rpc-server]
port = 8648
```

## Supported RPC Methods

Only these 11 methods are exposed (all others are rejected):

1. `getAccountByAddress(address)`
2. `getTransactionsByAddress(address, max, startAt)`
3. `getTransactionByHash(hash)`
4. `getTransactionsByBlockNumber(blockNumber)`
5. `getTransactionsByBatchNumber(batchNumber)`
6. `getBlockNumber()`
7. `getBatchNumber()`
8. `getBlockByNumber(blockNumber, includeTransactions)`
9. `isConsensusEstablished()`
10. `subscribeForHeadBlock()` — WebSocket subscription, streams full blocks on head changes
11. `subscribeForHeadBlockHash()` — WebSocket subscription, streams block hashes on head changes

## Testing

```bash
cargo nextest run --features=nimiq-zkp-component/test-prover   # Run tests (use nextest, not cargo test)
cargo test --doc                                                # Doctests (nextest doesn't support these)
```

Run a single test: `cargo nextest run -p nimiq-consensus test_name`

Use `nimiq_test_log::test` instead of the standard `#[test]`:
```rust
use nimiq_test_log::test;
#[test(tokio::test)]
async fn my_test() { ... }
```

## Linting & Formatting

```bash
cargo clippy --release --all-features       # Lint
cargo +nightly fmt --all -- --check         # Format check (requires nightly)
cargo check --all-features --tests --benches # Type-check everything
```

Rustfmt config: `style_edition = "2021"`, `group_imports = "StdExternalCrate"`, `imports_granularity = "Crate"`.

## Required Wrappers (enforced by clippy.toml)

These are **mandatory** — clippy will reject direct use of the originals. All wrappers exist for WASM compatibility.

| Instead of | Use |
|---|---|
| `tokio::task::spawn` | `nimiq_utils::spawn` |
| `tokio::task::spawn_local` | `nimiq_utils::spawn_local` |
| `tokio::time::sleep` / `sleep_until` | `nimiq_time::sleep` / `sleep_until` |
| `tokio::time::timeout` | `nimiq_time::timeout` |
| `tokio::time::interval` / `interval_at` | `nimiq_time::interval` |
| `futures::executor::block_on` | `tokio::runtime::Handle::current().block_on` |
| `FuturesUnordered` / `FuturesOrdered` / `SelectAll` | `nimiq_utils::stream::{FuturesUnordered, FuturesOrdered, SelectAll}` |
| `futures_timer::Delay` | `nimiq_time::sleep` |
| `gloo_timers::*` | `nimiq_time::*` equivalents |

## Architecture

Lightweight fork of Nimiq Albatross — a proof-of-stake blockchain in Rust. This fork removes validator, mempool, wallet, metrics, and eth-interface modules. It keeps only the consensus engine, blockchain storage, and a minimal RPC server.

**Key crates in this fork:**

- **client** — standalone node binary (the only build target)
- **consensus** — core consensus protocol
- **blockchain** — chain storage, validation, and history indexing
- **rpc-server** / **rpc-interface** — JSON-RPC API (only `BlockchainDispatcher`)
- **lib** (`nimiq-lib`) — umbrella crate wiring everything together
- **primitives/** — core types: `account`, `block`, `transaction`, `trie`, `mmr`
- **network-libp2p** — P2P networking layer
- **zkp-component** — zero-knowledge proof verification (needed for sync)
- **database** — persistent storage (MDBX backend)

**Removed from the official repo:**

- Validator / block production
- Mempool / transaction pool
- Wallet store
- Metrics server (Prometheus)
- Eth-compatible RPC interface
- ZKP prover (only verifier kept)

## Feature Flags

Key features on `nimiq-lib`:
- `full-consensus` (default) — full blockchain + database + DHT
- `rpc-server` — JSON-RPC server with `BlockchainDispatcher`
- `database-storage` — persistent DB layer

Removed features: `validator`, `wallet`, `metrics-server`, `zkp-prover`, `parallel`.

## Storage Optimizations

- **ValidityStore disabled** — tracks transactions in the validity window for mempool replay prevention. Not needed without a mempool.
- **Post-epoch MMR pruning** — Merkle Mountain Range proof nodes are deleted after epoch finalization. Transaction data is retained; only proof nodes are discarded.
- **zstd compression** — `HistoricTransaction`, `Block`, and `MicroBlock` values are zstd-compressed before writing to MDBX. A magic byte prefix (`0xFF`) distinguishes compressed from uncompressed data, providing backward compatibility with existing databases. Values smaller than 64 bytes are stored uncompressed. Compression utilities live in `nimiq-database-value::compressed`.

## Toolchain

- **Rust edition:** 2024
- **MSRV:** 1.88.0
- **Nightly:** required only for `rustfmt`

## Workspace Lint Allowances

`clippy::large_enum_variant`, `clippy::result_large_err`, `clippy::too_many_arguments`, `clippy::type_complexity` are allowed project-wide. `unused_qualifications` is warned.

## Crypto Crate Optimization

In dev and test profiles, crypto crates (bls, zkp, arkworks) are compiled with `opt-level = 2` for acceptable performance. The workspace patches arkworks `algebra` and `r1cs-std` with Nimiq forks.
