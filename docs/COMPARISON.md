# RSS comparison: native-lsp vs Node.js LSP

Measured on this cloud agent VM (Linux, `/proc/<pid>/status` `VmRSS`) after
`cargo build --release`. Both servers speak the same stdio JSON-RPC surface
(initialize, `didOpen`, hover, park sleep). The Node server is
`compare/node-lsp.mjs` (no npm deps). Default fixtures are generated PHP classes
with `add_action` / `add_filter` hooks. `COMPARE_MODE=mixed` opens ten files of
different languages from `testdata/mixed/` in one process. Common languages
use a **tree-sitter CST on the active tab only**; hidden tabs nap. SQL is
line-scan (no tree-sitter 0.22 crate). Release binary is **5.0 MB**.

This is **not** Intelephense. It is the V8 runtime tax plus a heap document
store vs a static Rust binary with interned `u32` names. That is the first
claim from the research: drop Node before arguing about indexes.

## 80 PHP files

| stage | native-lsp | node-lsp | delta |
| --- | ---: | ---: | ---: |
| idle after initialize | 2.32 MB | 45.97 MB | native saves 43.64 MB |
| after didOpen all files + hover | 3.48 MB | 47.15 MB | native saves 43.67 MB |
| after park sleep | 3.48 MB | 47.18 MB | native saves 43.71 MB |

Hover: native 0 ms, node 0 ms.

Each `didOpen` naps the previous file, so 80 PHP buffers cost **one CST**
(~3.5 MB), not 80 trees. Before the tab split, 80 CSTs were 4.75 MB.

## 200 PHP files

| stage | native-lsp | node-lsp | delta |
| --- | ---: | ---: | ---: |
| idle after initialize | 2.30 MB | 45.96 MB | native saves 43.66 MB |
| after didOpen all files + hover | 3.64 MB | 54.44 MB | native saves 50.80 MB |
| after park sleep | 3.64 MB | 54.49 MB | native saves 50.86 MB |

Hover: native 0 ms, node 0 ms. Node grows ~8 MB on 200 files; native barely
moves because only the last tab keeps a tree.

## Notes

- Native stays **~3.5–6.5 MB** with lazy grammars + one CST, under the 80 MB bar.
- Node idle is already **~46 MB** (V8 floor).
- Park sleep did not shrink RSS: dropping parser objects does not unmap
  grammar pages. The win is still not paying a 45 MB runtime.
- Re-run: `cargo build --release --bin native-lsp --bin compare-rss && ./target/release/compare-rss`

Replay: `COMPARE_FILES=200 ./target/release/compare-rss`

## Active-tab split (grammars on demand)

`COMPARE_MODE=tabs` is one process. Open PHP, then JS, then the rest of
`testdata/mixed/`. Only the latest tab keeps a CST. Grammars load when first
needed and stay mapped.

| stage | native RSS | CST | grammars | node RSS | delta |
| --- | ---: | ---: | --- | ---: | --- |
| idle after initialize | 2.34 MB | 0 | — | 45.80 MB | native saves 43.46 MB |
| open PHP (1 CST) | 3.37 MB | 1 | php | 45.85 MB | native saves 42.48 MB |
| open JS (PHP napped, JS CST) | 3.76 MB | 1 | javascript, php | 45.90 MB | native saves 42.14 MB |
| open remaining 8 langs (1 CST) | 6.38 MB | 1 | 9 grammars | 46.11 MB | native saves 39.73 MB |
| pin PHP active, hide others | 6.44 MB | 1 | 9 grammars | 46.60 MB | native saves 40.16 MB |
| park (drop CSTs + parsers) | 6.44 MB | 0 | — | 46.61 MB | native saves 40.18 MB |

PHP grammar ~1 MB. Each extra grammar is a few hundred KB of faulted pages.
Visiting all nine still sits at **6.4 MB vs Node 46 MB**. Keeping every CST
would have been the rust-analyzer mistake; the binary can hold the grammars
because the trees do not.

Replay: `COMPARE_MODE=tabs ./target/release/compare-rss`

## Mixed languages (10 files, one process)

`COMPARE_MODE=mixed` opens `testdata/mixed/` — PHP, JavaScript, TypeScript, HTML,
CSS, JSON, YAML, SQL, Python, Rust — in a **single** native-lsp (and Node)
process. Hover and `documentSymbol` are probed per file (hidden tabs re-parse
symbols, no second CST). `memoryReport`: **1 CST**, grammars
`css, html, javascript, json, php, python, rust, typescript, yaml`. SQL is
line-scan.

| file | language | native symbols | native hover | node symbols | node hover |
| --- | --- | ---: | --- | ---: | --- |
| `01-plugin.php` | php | 6 | php class Mixed_Plugin | 6 | php class Mixed_Plugin |
| `02-widget.js` | javascript | 5 | javascript class CartWidget | 3 | javascript class CartWidget |
| `03-api.ts` | typescript | 7 | typescript type UserId | 5 | typescript type UserId |
| `04-page.html` | html | 7 | html tag hero | 6 | html tag hero |
| `05-theme.css` | css | 4 | css rule hero | 4 | css rule hero |
| `06-package.json` | json | 8 | json key name | 8 | json key name |
| `07-compose.yaml` | yaml | 10 | yaml key services | 5 | yaml key services |
| `08-schema.sql` | sql | 3 | sql table posts | 3 | sql table posts |
| `09-app.py` | python | 5 | python class Store | 5 | python class Store |
| `10-lib.rs` | rust | 6 | rust type Kind | 6 | rust type Kind |

| stage | native-lsp | node-lsp | delta |
| --- | ---: | ---: | ---: |
| idle after initialize | 2.30 MB | 45.88 MB | native saves 43.58 MB |
| after didOpen 10 languages + hover | 6.56 MB | 46.34 MB | native saves 39.78 MB |
| after park sleep | 6.56 MB | 46.36 MB | native saves 39.79 MB |

Tree-sitter finds methods/nested keys the line scanner missed (JS 5 vs 3,
YAML 10 vs 5). Interned unique names on native: 78 (includes WordPress stubs).

Replay: `COMPARE_MODE=mixed ./target/release/compare-rss`

## Same mixed files, real editors

Installed on this VM: Neovim 0.12.5, Helix 25.07.1, VS Code 1.134, Emacs 29.3
(already present). `compare-hosts` opens `testdata/mixed/` in each editor and
swaps only the language-server child. IDE RSS is the editor process tree
**minus** the LSP. That is the fair host A/B.

| host | LSP | IDE | LSP | total |
| --- | --- | ---: | ---: | ---: |
| native-ide | native | 2.45 MB | 2.31 MB | **4.76 MB** |
| native-ide | node | 2.58 MB | 47.67 MB | 50.25 MB |
| neovim | native | 12.55 MB | 2.32 MB | **14.88 MB** |
| neovim | node | 12.70 MB | 46.21 MB | 58.90 MB |
| helix | native | 36.65 MB | 2.21 MB | **38.86 MB** |
| helix | node | 36.57 MB | 45.99 MB | 82.56 MB |
| emacs | native | 67.44 MB | 2.31 MB | **69.75 MB** |
| emacs | node | 67.48 MB | 45.77 MB | 113.25 MB |
| vscode | native | 1749.68 MB | 2.99 MB | 1752.68 MB |
| vscode | node | 1710.14 MB | 46.09 MB | 1756.22 MB |

Across every host the language server stays **~3 MB native vs ~46 MB Node**
(tree-sitter PHP raises the native floor a little). The editor floor is what
changes. VS Code’s Electron process tree is ~1.7 GB here, so the LSP gap is
real but small next to the IDE. Neovim is the other end: a real LSP client at
~13 MB plus a ~3 MB native server.

VS Code rows are from this branch (`COMPARE_HOSTS=vscode`). Other hosts are
the previous A/B; the server child is the same order of magnitude.

## Why VS Code is ~1.7 GB (not native-lsp)

The native server in that run was **2.99 MB**. The rest is the editor.
`compare-hosts` attributes every process whose cmdline contains the unique
`--user-data-dir` temp dir to “vscode”:

### vscode + native-lsp (role totals)

| role | processes | RSS |
| --- | ---: | ---: |
| node utility (Electron) | 3 | 597.98 MB |
| renderer (Monaco / workbench) | 1 | 561.61 MB |
| main (Electron) | 1 | 224.05 MB |
| extensionHost (Node) | 1 | 186.81 MB |
| gpu-process (Chromium) | 1 | 93.75 MB |
| network utility | 1 | 81.17 MB |
| crashpad | 1 | 4.31 MB |
| language-server (native-lsp) | 1 | **2.99 MB** |

1. **Electron = Chromium.** Main + renderer (Monaco) + GPU + crashpad plus a
   handful of Node-shaped utility processes. That is most of the 1.75 GB.
2. **The extension host is still Node (~187 MB).**
   `editors/vscode/extension.js` uses `vscode-languageclient`. Native-lsp only
   replaces the language-server **child**. You still pay for a Node extension
   host.
3. **Workbench cost is fixed.** Opening ten mixed files still starts the full
   VS Code UI. Swap native-lsp for node-lsp and IDE RSS stays ~1.7 GB; swap
   Neovim for VS Code and it jumps by ~1.7 GB.

Replay the per-process dump:

```bash
COMPARE_HOSTS=vscode ./target/release/compare-hosts
```

Replay:

```bash
cargo build --release --bin native-lsp --bin native-ide --bin compare-hosts
./target/release/compare-hosts
```

## Same IDE, swap the LSP

Headless `compare-rss` is a fair **LSP** A/B. Users feel **editor + server**.
That is only fair if the editor is the same process and we report three
numbers: IDE RSS, LSP RSS, total.

`native-ide` holds the editor constant. Ten mixed-language tabs, hover,
`documentSymbol`, and `$/nativeLsp/documentVisibility` on tab switch (not
`didClose`). It is not VS Code. This is not Intelephense. A homemade IDE vs
Cursor + Intelephense would mix editor RAM, extensions, and analysis depth.

| stack | IDE | LSP | total |
| --- | ---: | ---: | ---: |
| native-ide + native-lsp | 2.45 MB | 2.32 MB | **4.77 MB** |
| native-ide + node-lsp | 2.56 MB | 47.68 MB | **50.23 MB** |

| stage | native total | node total | delta |
| --- | ---: | ---: | ---: |
| idle (IDE buffers + LSP initialize) | 4.61 MB | 48.38 MB | native saves 43.77 MB |
| after didOpen all tabs | 4.64 MB | 48.62 MB | native saves 43.98 MB |
| after cycling tabs + hover | 4.77 MB | 50.23 MB | native saves 45.46 MB |

The IDE floor stayed ~2.5 MB on both runs. The ~45 MB gap is the Node LSP
child, not the editor.

Replay:

```bash
cargo build --release --bin native-lsp --bin native-ide --bin compare-rss
COMPARE_MODE=ide ./target/release/compare-rss
```
