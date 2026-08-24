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
