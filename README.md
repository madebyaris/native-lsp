# native-lsp

The language server you need, without Node.js and without the RAM.

This repo is a **native Language Server Protocol (LSP)** implementation: one static binary, stdio JSON-RPC, built to stay small. Research is in [`docs/RESEARCH.md`](docs/RESEARCH.md). There is no server implementation yet.

## Why

Editors such as Cursor and VS Code typically spawn one Node.js language server per language. An empty `node` process already sits at ~30–50 MB (V8). Intelephense on a WordPress / Symfony / Magento tree commonly uses **400 MB–2 GB**. HTML, CSS, and JSON each add another Node process.

Native code can drop that runtime tax. It does not automatically drop RAM: `rust-analyzer` and `clangd` prove a full in-memory semantic model still reaches gigabytes. The design here is **open-file parse + compact on-disk index**, not “keep the whole workspace CST hot”.

## Decisions (from the research)

| Topic | Choice |
| --- | --- |
| Language | **Rust** (Zig and C++ evaluated; Go noted because TypeScript 7 went that way) |
| Protocol | LSP 3.17+ over stdio, `lsp-server` + `lsp-types` |
| RAM model | Arena for the current file, mmap symbol index, no vendor/`node_modules` by default |
| VRAM | Not for the symbol table (Apple Silicon is the same DRAM; CUDA lookups are too slow per keystroke). Optional later embedding sidecar |
| Host profiles | One binary; skip GPU steps on machines that do not have them |
| Lifecycle | IDE tells us tab-active / idle / sleep; we do not infer it from `didOpen` |
| First language | PHP, WordPress-shaped (`wp-includes` stubs, hooks) |
| Success bar | Idle RSS **&lt; 80 MB** on a typical WP plugin fixture |
| Not in scope | TypeScript type checking (`tsgo` already exists), cloning Intelephense |

## Status

- [x] Language and architecture research
- [x] Host profiles + IDE-driven sleep/wake (see `docs/RESEARCH.md` §8)
- [ ] Hello-world server (`initialize` / `shutdown` + RSS report)
- [ ] Compact index + tree-sitter PHP
- [ ] WordPress stubs and hook intelligence
- [ ] HTML / CSS / JSON in the same process

## License

TBD.
