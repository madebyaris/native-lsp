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
- [x] tree-sitter CST for common languages on the **active tab only** (nap drops the tree)
- [x] Production-style VS Code client vs stock html/css/json/tsserver (`COMPARE_PROFILE=real`)

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
# one process, load grammars as tabs open, keep one CST:
COMPARE_MODE=tabs ./target/release/compare-rss
```

Success bar from research: native idle/open RSS **under 80 MB**.

Latest run (Linux, release, active-tab CST): native **~3.5 MB** (80 PHP
files) / **~3.6 MB** (200 PHP files) vs Node **~47–54 MB**; visiting all
nine tree-sitter grammars in one process is **~6.4 MB vs ~46 MB**. Full
table: [`docs/COMPARISON.md`](docs/COMPARISON.md).

Real VS Code, WordPress-shaped plugin (`testdata/wp-plugin/`), same
`executeHoverProvider` path the UI uses:

```bash
COMPARE_PROFILE=real ./target/release/compare-hosts
```

| stack | language servers | files with hover |
| --- | ---: | ---: |
| vscode + native-lsp | **5.59 MB** (1 process) | 12 / 12 |
| vscode + stock html/css/json/tsserver | **747 MB** (5 Node processes) | 4 / 12 |

Stock hover works for JS, TS, CSS, and JSON. PHP, YAML, SQL, and HTML have
no built-in language server (PHP completions are word lists). Native-lsp is
still symbols/hover/completion from a CST, not Intelephense or tsserver.

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
mixed files. The language server stays **~3 MB native vs ~46 MB Node** in every
host; VS Code’s Electron tree is ~1.7 GB so the editor dominates.

Production-like A/B (open a plugin folder, probe through the editor APIs):

```bash
COMPARE_PROFILE=real ./target/release/compare-hosts
```

A homemade IDE vs Cursor + Intelephense would mix editor RAM, extensions, and
analysis depth. That is a product comparison, not an LSP comparison.

## Why VS Code looks huge with this LSP

It is not native-lsp. The last host A/B had **native-lsp at ~3 MB** (PHP
tree-sitter loaded) inside a **~1.75 GB** VS Code process tree. `compare-hosts`
sums every process whose cmdline contains the unique `--user-data-dir` marker:

- Chromium **main** (~224 MB) + **renderer / Monaco** (~562 MB) + **GPU** (~94 MB)
- Extra Electron **Node utilities** (~600 MB) + **extension host** (~187 MB)
- The **extension host is still Node** (`vscode-languageclient` in
  `editors/vscode/`). Native LSP only replaces the **server** child (~3 MB).
- Opening ten files still loads the full workbench.

Replay just that dump: `COMPARE_HOSTS=vscode ./target/release/compare-hosts`

## Language support

This is **not** Intelephense + tsserver + vscode-html/css/json. One process,
symbols + hover + completion from interned names. No typechecker, no
workspace goto-def index, no diagnostics.

| Language | Parser | What works |
| --- | --- | --- |
| PHP | tree-sitter (line-scan fallback) | class, method, function, `add_action` / `add_filter` |
| JavaScript | tree-sitter | class, function, method, const/let/var fn |
| TypeScript | tree-sitter | JS plus interface, type, enum |
| HTML | tree-sitter | id, class, custom elements |
| CSS | tree-sitter | selectors, `@keyframes` |
| JSON | tree-sitter | object keys |
| YAML | tree-sitter | mapping keys |
| SQL | line-scan | `CREATE TABLE/VIEW/INDEX/FUNCTION` (no 0.22 grammar crate) |
| Python | tree-sitter | class, def |
| Rust | tree-sitter | fn, struct, enum, impl, trait, mod, const |
| Go, Java, C/C++, Ruby, Vue, Markdown, Shell, … | none | plaintext; empty symbols |

One binary, **lazy grammars**, **one CST**. `didOpen` makes that tab Active and naps the previous one. `$/nativeLsp/documentVisibility` does the same on a real tab switch. Hover of a hidden tab re-parses symbols without keeping a second tree. `park` drops CSTs and parser instances (grammar pages may stay mapped).

This is still not a typechecker. SQL stays line-scan because there is no tree-sitter 0.22-compatible crate. Go/Java/C and friends are not in the binary yet — adding them is more compile time and more file-backed grammar pages, not more Node processes.

## License

TBD.
