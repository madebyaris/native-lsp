# RSS comparison: native-lsp vs Node.js LSP

Measured on this cloud agent VM (Linux, `/proc/<pid>/status` `VmRSS`) after
`cargo build --release`. Both servers speak the same stdio JSON-RPC surface
(initialize, `didOpen`, hover, park sleep). The Node server is
`compare/node-lsp.mjs` (no npm deps). Default fixtures are generated PHP classes
with `add_action` / `add_filter` hooks. `COMPARE_MODE=mixed` opens ten files of
different languages from `testdata/mixed/` in one process.

This is **not** Intelephense. It is the V8 runtime tax plus a heap document
store vs a static Rust binary with interned `u32` names. That is the first
claim from the research: drop Node before arguing about indexes.

## 80 PHP files

| stage | native-lsp | node-lsp | delta |
| --- | ---: | ---: | ---: |
| idle after initialize | 2.20 MB | 45.71 MB | native saves 43.51 MB |
| after didOpen all files + hover | 2.33 MB | 46.88 MB | native saves 44.55 MB |
| after park sleep | 2.33 MB | 46.88 MB | native saves 44.55 MB |

Hover: native 0 ms, node 1 ms.

## 200 PHP files

| stage | native-lsp | node-lsp | delta |
| --- | ---: | ---: | ---: |
| idle after initialize | 2.13 MB | 45.86 MB | native saves 43.73 MB |
| after didOpen all files + hover | 2.39 MB | 54.40 MB | native saves 52.02 MB |
| after park sleep | 2.39 MB | 53.99 MB | native saves 51.60 MB |

Hover: native 0 ms, node 2 ms.

## Notes

- Native stays **~2.3 MB**, under the 80 MB research bar by a wide margin —
  including ten languages in one process.
- Node idle is already **~46 MB** (V8 floor). Opening 200 PHP files adds ~8 MB more.
- Park sleep did not shrink RSS here: dropping parsed symbol vecs does not
  return pages to the OS at this size. The win is not allocating them on a
  45 MB runtime in the first place.
- Re-run: `cargo build --release --bin native-lsp --bin compare-rss && ./target/release/compare-rss`

Replay: `COMPARE_FILES=200 ./target/release/compare-rss`

## Mixed languages (10 files, one process)

`COMPARE_MODE=mixed` opens `testdata/mixed/` — PHP, JavaScript, TypeScript, HTML,
CSS, JSON, YAML, SQL, Python, Rust — in a **single** native-lsp (and Node)
process. Hover and `documentSymbol` are probed per file.

| file | language | native symbols | native hover | node symbols | node hover |
| --- | --- | ---: | --- | ---: | --- |
| `01-plugin.php` | php | 6 | php class Mixed_Plugin | 6 | php class Mixed_Plugin |
| `02-widget.js` | javascript | 3 | javascript class CartWidget | 3 | javascript class CartWidget |
| `03-api.ts` | typescript | 5 | typescript type UserId | 5 | typescript type UserId |
| `04-page.html` | html | 6 | html tag hero | 6 | html tag hero |
| `05-theme.css` | css | 4 | css rule hero | 4 | css rule hero |
| `06-package.json` | json | 8 | json key name | 8 | json key name |
| `07-compose.yaml` | yaml | 5 | yaml key services | 5 | yaml key services |
| `08-schema.sql` | sql | 3 | sql table posts | 3 | sql table posts |
| `09-app.py` | python | 5 | python class Store | 5 | python class Store |
| `10-lib.rs` | rust | 6 | rust type Kind | 6 | rust type Kind |

| stage | native-lsp | node-lsp | delta |
| --- | ---: | ---: | ---: |
| idle after initialize | 2.16 MB | 45.82 MB | native saves 43.66 MB |
| after didOpen 10 languages + hover | 2.26 MB | 46.27 MB | native saves 44.02 MB |
| after park sleep | 2.26 MB | 46.29 MB | native saves 44.03 MB |

Ten languages did not move native RSS off the ~2.2 MB floor. Node is still the
V8 idle tax. Interned unique names on native: 71 (includes WordPress stubs).
Node’s “interned” counter is symbol instances (51), not unique strings.

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
| vscode | native | 1830.16 MB | 2.23 MB | 1832.39 MB |
| vscode | node | 1715.34 MB | 45.97 MB | 1761.31 MB |

Across every host the language server stays **~2.3 MB native vs ~46 MB Node**.
The editor floor is what changes. VS Code’s Electron process tree is ~1.7 GB
here, so the LSP gap is real but small next to the IDE. Neovim is the other
end: a real LSP client at ~13 MB plus a 2.3 MB native server.

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
