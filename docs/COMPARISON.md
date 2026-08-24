# RSS comparison: native-lsp vs Node.js LSP

Measured on this cloud agent VM (Linux, `/proc/<pid>/status` `VmRSS`) after
`cargo build --release`. Both servers speak the same stdio JSON-RPC surface
(initialize, `didOpen`, hover, park sleep). The Node server is
`compare/node-lsp.mjs` (no npm deps). Fixture files are generated PHP classes
with `add_action` / `add_filter` hooks.

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

- Native stays **~2.3 MB**, under the 80 MB research bar by a wide margin.
- Node idle is already **~46 MB** (V8 floor). Opening 200 files adds ~8 MB more.
- Park sleep did not shrink RSS here: dropping parsed symbol vecs does not
  return pages to the OS at this size. The win is not allocating them on a
  45 MB runtime in the first place.
- Re-run: `cargo build --release --bin native-lsp --bin compare-rss && ./target/release/compare-rss`

Replay: `COMPARE_FILES=200 ./target/release/compare-rss`
