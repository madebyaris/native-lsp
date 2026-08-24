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
- [x] Tiny native workbench (`native-ide`) so the editor stays constant
- [x] Host A/B in Neovim, Emacs, Helix, and VS Code (`compare-hosts`)
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

Latest run (Linux, release): native **~2.3 MB** vs Node **~47–54 MB** on PHP
fixtures; **~2.3 MB vs ~46 MB** with ten languages in one process. Full table:
[`docs/COMPARISON.md`](docs/COMPARISON.md).

## Fair comparison: same IDE, swap the LSP

Headless `compare-rss` is already a fair **LSP** A/B (same messages, measure
only the server). What users feel is **editor + server**. That is only fair if
the editor is the same process and we report three numbers: IDE RSS, LSP RSS,
total.

`native-ide` is that editor: mixed-language tabs, hover, document symbols, and
`$/nativeLsp/documentVisibility` on tab switch (not `didClose`). It is not
VS Code, and this is not Intelephense.

```bash
cargo build --release --bin native-lsp --bin native-ide --bin compare-rss --bin compare-hosts
./target/release/native-ide --lsp native --once
COMPARE_MODE=ide ./target/release/compare-rss
./target/release/compare-hosts
```

`compare-hosts` drives Neovim, Emacs/Eglot, Helix, and VS Code with the same
mixed files. The language server stays ~2.3 MB native vs ~46 MB Node in every
host; VS Code’s Electron tree is ~1.7 GB so the editor dominates.

A homemade IDE vs Cursor + Intelephense would mix editor RAM, extensions, and
analysis depth. That is a product comparison, not an LSP comparison.

## License

TBD.
