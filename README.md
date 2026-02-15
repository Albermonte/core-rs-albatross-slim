# Nimiq Lightweight History Node

A stripped-down fork of [nimiq/core-rs-albatross](https://github.com/nimiq/core-rs-albatross) that runs as a dedicated **read-only history node**. It syncs the full blockchain and exposes only 11 RPC methods for querying accounts, transactions, and blocks.

This is not a general-purpose Nimiq node. It cannot produce blocks, relay transactions, or manage wallets.

---

### Table of Contents
- [Supported RPC Methods](#supported-rpc-methods)
- [Hardware Requirements](#hardware-requirements)
- [Installation](#installation)
- [Configuration](#configuration)
- [Running](#running)
- [Example RPC Calls](#example-rpc-calls)
- [Differences from Official Repository](#differences-from-official-repository)
- [History Nodes First Start](#history-nodes-first-start)
- [License](#license)

## Supported RPC Methods

| Method | Parameters | Description |
|--------|------------|-------------|
| `getBlockNumber` | none | Returns the current block height |
| `getBatchNumber` | none | Returns the current batch number |
| `getBlockByNumber` | `blockNumber`, `includeTransactions` | Returns a block by its number |
| `getAccountByAddress` | `address` | Returns account state for an address |
| `getTransactionByHash` | `hash` | Returns a transaction by its hash |
| `getTransactionsByAddress` | `address`, `max`, `startAt` | Returns transactions for an address (paginated) |
| `getTransactionsByBlockNumber` | `blockNumber` | Returns transactions in a given block |
| `getTransactionsByBatchNumber` | `batchNumber` | Returns transactions in a given batch |
| `isConsensusEstablished` | none | Returns whether consensus is established |
| `subscribeForHeadBlock` | none | WebSocket subscription — streams full blocks on head changes |
| `subscribeForHeadBlockHash` | none | WebSocket subscription — streams block hashes on head changes |

All other RPC methods are rejected by the server.

## Hardware Requirements

| Resource | Minimum | Recommended |
|----------|---------|-------------|
| Memory | 16 GB RAM | Higher recommended |
| CPU | 4 vCPUs | 8 vCPUs |
| Storage | 1 TB (2 TB with indexing) | SSD required |
| Network | High-speed, reliable connection | Good I/O performance |

Storage usage starts at a few gigabytes and grows linearly with blockchain size over time. This fork reduces storage compared to the official history node by pruning MMR proof data and disabling the validity store.

## Installation

1. Install Rust 1.88.0 or later from [rustup.rs](https://rustup.rs/) and the following system packages:
   - `clang`
   - `cmake`
   - `libssl-dev` (Debian/Ubuntu) or `openssl-devel` (Fedora/Red Hat)
   - `pkg-config`

2. Clone and build:
```bash
git clone <this-repo-url>
cd core-rs-albatross
cargo build -p nimiq-client --release
```

The binary is at `target/release/nimiq-client`.

You can also run directly without installing:
```bash
cargo run --release --bin nimiq-client
```

3. Optionally install system-wide:
```bash
cargo install --path client/
```

## Configuration

Create a TOML config file (e.g. `client.toml`). You can start from the [example config](lib/src/config/config_file/client.example.toml).

Key settings for this fork:

```toml
[consensus]
network = "main-albatross"    # or "test-albatross"
sync_mode = "history"         # Required — must be "history"
index_history = true          # Required for getTransactionsByAddress

[database]
path = "./db"

[network]
listen_addresses = ["/ip4/0.0.0.0/tcp/8443/ws"]

[rpc-server]
bind = "127.0.0.1"
port = 8648
# cors_domains = ["*"]       # Optional: allow CORS from any origin
# username = "user"          # Optional: basic auth
# password = "pass"

[log]
level = "info"
timestamps = true
statistics = 10               # Log peer/block stats every N seconds (0 = disable)
```

### Required settings

- **`sync_mode = "history"`** — the node must sync in history mode to store transaction data.
- **`index_history = true`** (under `[consensus]`) — enables the address-to-transaction index needed by `getTransactionsByAddress`.

### Port configuration

Ensure your firewall allows traffic on port **8443/tcp** (P2P) and **8648/tcp** (RPC).

## Running

```bash
# With default config location (~/.nimiq/client.toml)
./target/release/nimiq-client

# With explicit config file
./target/release/nimiq-client --config client.toml

# Or run directly via cargo
cargo run --release --bin nimiq-client
cargo run --release --bin nimiq-client -- --config client.toml
```

The node connects to the network, syncs the blockchain, and starts serving RPC requests once synced.

## Example RPC Calls

```bash
# Get current block number
curl -s -X POST http://127.0.0.1:8648 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"getBlockNumber","params":[],"id":1}'

# Get block by number (with transactions)
curl -s -X POST http://127.0.0.1:8648 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"getBlockByNumber","params":[100, true],"id":2}'

# Get account by address
curl -s -X POST http://127.0.0.1:8648 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"getAccountByAddress","params":["NQ07 0000 0000 0000 0000 0000 0000 0000 0000"],"id":3}'

# Get transaction by hash
curl -s -X POST http://127.0.0.1:8648 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"getTransactionByHash","params":["abcdef1234..."],"id":4}'

# Get transactions by address (paginated, max 100)
curl -s -X POST http://127.0.0.1:8648 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"getTransactionsByAddress","params":["NQ07 0000 0000 0000 0000 0000 0000 0000 0000", 100],"id":5}'

# Get transactions by block number
curl -s -X POST http://127.0.0.1:8648 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"getTransactionsByBlockNumber","params":[100],"id":6}'

# Get transactions by batch number
curl -s -X POST http://127.0.0.1:8648 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"getTransactionsByBatchNumber","params":[1],"id":7}'

# Get current batch number
curl -s -X POST http://127.0.0.1:8648 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"getBatchNumber","params":[],"id":8}'

# Check if consensus is established
curl -s -X POST http://127.0.0.1:8648 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"isConsensusEstablished","params":[],"id":9}'
```

## Differences from Official Repository

| Feature | Official | This Fork |
|---------|----------|-----------|
| Block production (validator) | Yes | Removed |
| Transaction mempool | Yes | Removed |
| Wallet management | Yes | Removed |
| Metrics server (Prometheus) | Yes | Removed |
| Eth-compatible RPC | Yes | Removed |
| Full RPC API (~30 methods) | Yes | 11 methods only |
| ZKP prover | Yes | Removed (verifier only) |
| ValidityStore (replay protection) | Active | Disabled |
| MMR proof storage | Persistent | Pruned after epoch finalization |
| Sync modes | Full, Light, History | History only |

### Storage optimizations

1. **ValidityStore disabled** — the validity window tracker (used by the mempool for replay prevention) is not written to. Saves 2 database tables.
2. **Post-epoch MMR pruning** — Merkle Mountain Range proof nodes are deleted after each epoch is finalized. Transaction data is kept for RPC queries; only proof nodes (used for generating inclusion proofs) are discarded.
3. **zstd compression** — `HistoricTransaction`, `Block`, and `MicroBlock` values are zstd-compressed before writing to MDBX. A magic byte prefix (`0xFF`) distinguishes compressed from uncompressed data, providing backward compatibility with existing databases. Values smaller than 64 bytes are stored uncompressed.

### Removed crates/modules

- `rpc-server/src/dispatchers/` — all dispatchers except `blockchain` removed
- `rpc-server/src/eth_interface/` — entire Eth-compatible interface removed
- `rpc-server/src/wallets.rs` — wallet store removed
- `rpc-interface/` — only `blockchain` interface module kept

### Removed nimiq-lib features

`validator`, `wallet`, `metrics-server`, `zkp-prover`, `parallel`

## History Nodes First Start

For the first start of a history node on **mainnet**, you must set the environment variable `NIMIQ_OVERRIDE_MAINNET_CONFIG` to point to a genesis configuration file. This file can be downloaded from:

- **Nimiq IPFS gateway**: https://ipfs.nimiq.io/ipfs/QmWcRRRw4FaKRrznMFt6KemAM35uo9QknMkDaeBzTod33R
- **Via torrent**:
    ```
    magnet:?xt=urn:btih:566cec0c350fca917cf5abb00c7dbe8c70884306&dn=nimiq-genesis-main-albatross.toml&tr=https%3A%2F%2Ftorrents.nimiq.io%2Fannounce
    ```

Run:
```bash
NIMIQ_OVERRIDE_MAINNET_CONFIG=/path/to/nimiq-genesis-main-albatross.toml \
  cargo run --release -p nimiq-client
```

This is required **only for the first start**. Subsequent restarts don't need the file or environment variable.

## License

This project is licensed under the [Apache License 2.0](./LICENSE.md).
