# native-lsp

The language server you need, without Node.js and without the RAM.

A **native Language Server Protocol (LSP)** server: one static binary, stdio JSON-RPC, interned `u32` symbol IDs, disk snapshot, IDE-driven sleep. Research: [`docs/RESEARCH.md`](docs/RESEARCH.md).

## Status

- [x] Language and architecture research
- [x] Host profiles + IDE-driven sleep/wake
- [x] Compact intern snapshot on disk
- [x] PHP / WordPress-shaped symbols (`class`, `function`, `add_action` / `add_filter`)
- [x] Ten-language scanners in one process (PHP, JS, TS, HTML, CSS, JSON, YAML, SQL, Python, Rust)
- [x] RSS comparison vs a Node.js LSP with the same protocol
- [ ] tree-sitter PHP CST

## Run the server

```bash
cargo build --release --bin native-lsp
./target/release/native-lsp
```

Point Cursor / VS Code / Neovim at that binary over stdio. Capabilities: full document sync, hover, completion, document symbols.

Custom protocol (optional client):

| Method | Kind | Meaning |
| --- | --- | --- |
| `nativeLsp/memoryReport` | request | RSS, intern count, open/parsed docs |
| `$/nativeLsp/sleep` | notification | `{ "depth": "nap" \| "park" }` — drop parsed symbols |
| `$/nativeLsp/wake` | notification | re-parse visible docs |
| `$/nativeLsp/documentVisibility` | notification | `{ "uri", "state": "active" \| "visible" \| "hidden" }` |

## RSS comparison

Same fixture, same LSP messages, `/proc/<pid>/status` `VmRSS`:

```bash
cargo build --release --bin native-lsp --bin compare-rss
./target/release/compare-rss
# optional: COMPARE_FILES=200 ./target/release/compare-rss
# ten files, ten languages, one process:
COMPARE_MODE=mixed ./target/release/compare-rss
```

Success bar from research: native idle/open RSS **under 80 MB**.

Latest run (Linux, release, same fixture): native **~2.3 MB** vs Node **~47–54 MB**. Full table: [`docs/COMPARISON.md`](docs/COMPARISON.md).

## License

TBD.
