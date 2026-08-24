# Native LSP research

**Status:** research complete, no implementation yet.
**Question:** can we replace Node.js language servers with a native binary (C++, Zig, or Rust) and actually use less RAM?
**Short answer:** yes — but only if we pick the right *architecture*. The language is maybe 20–40% of the RAM win. Indexing strategy is the rest.

**VRAM follow-up:** parking the index in GPU memory does **not** move the hurdle off system RAM on Apple Silicon (unified memory is one DRAM pool). On a discrete NVIDIA card it can hide RSS from the CPU, but it is the wrong place for go-to-definition / completion (single-key latency). VRAM is a later optional sidecar for **dense embedding search**, not for the symbol table.

---

## 1. Why Node.js LSPs feel heavy

A language server is a long-lived process. The editor talks to it over JSON-RPC (usually stdin/stdout). Every open workspace typically starts **one process per language**. A WordPress + JS project in Cursor/VS Code often has several Node servers at once:

| Server | Runtime | Typical RSS | What it does |
| --- | --- | --- | --- |
| Intelephense | Node / V8 | 400 MB – 2 GB | PHP semantics, index, completion |
| tsserver | Node / V8 | 200 MB – 1.5 GB | TypeScript typecheck + language service |
| vscode-html / css / json | Node / V8 | 40–120 MB each | Markup / style / schema |
| Empty `node` process | V8 | **~30–50 MB** | Runtime tax with no user code |

Three costs stack:

1. **Runtime floor.** V8 + libuv + JIT code cache sit at ~30–50 MB *before* any AST exists. Five Node LSPs is 150–250 MB of empty runtime.
2. **Object model.** Every AST node, map, and string is a heap object with hidden classes, pointers, and GC headers. A compact C struct of the same data is often 3–8× smaller.
3. **GC slack.** V8 keeps unused heap (new-space buffers, old-space after GC) so RSS is larger than live data. `intelephense.maxMemory` exists because the process hits the heap cap and dies (`FATAL ERROR: JavaScript heap out of memory`). Magento / Symfony / WordPress-with-vendor reports of 500 MB–2 GB are common.

**Important:** native does not magically fix this. `rust-analyzer` (Rust) routinely uses 600 MB–several GB. `clangd` (C++) uses ~2.7 GB of RAM for a Chromium index that is only ~550 MB on disk. The language dropped the V8 tax; the *full in-memory semantic model* put it back.

The project that actually hit the RAM goal recently is [rust-glancer](https://matklad.github.io/2026/08/21/rust-glancer.html) (matklad, Aug 2026): same protocol, frozen analysis on disk, page-in on query, target **&lt; 100 MB**. The lesson is architectural, not “rewrite it in X”.

---

## 2. What we would actually build

LSP is not a language. It is a **JSON-RPC contract** (current spec: 3.18):

```
editor  --textDocument/didOpen-->  server
editor  --textDocument/didChange-->  server
editor  --textDocument/completion-->  server  --> items
editor  --textDocument/definition-->  server  --> location
server  --textDocument/publishDiagnostics-->  editor
```

The protocol is language-neutral (URI + line/character). All the RAM lives in the **analysis engine** behind it:

```
┌──────────────────────────────────────────────────────────┐
│  JSON-RPC / LSP 3.17–3.18  (cheap, ~a few MB)            │
├──────────────────────────────────────────────────────────┤
│  Open documents + incremental parse  (must be in RAM)    │
├──────────────────────────────────────────────────────────┤
│  Symbol / reference index  (this is where GB appear)     │
├──────────────────────────────────────────────────────────┤
│  Type / semantic engine  (optional, most expensive)      │
└──────────────────────────────────────────────────────────┘
```

A useful native server can stop at layer 2 (syntax + local completion) and still beat Node on RAM. Full type-aware PHP/TS is a multi-year engine. Those are different products.

---

## 3. Language comparison: Rust vs Zig vs C++

Go is included because TypeScript 7 (`tsgo`) just chose it, and that decision is the best public write-up we have on “native compiler vs Node”.

### Scorecard (for *this* repo)

| Criterion | Rust | Zig | C++ | Go (not requested, but relevant) |
| --- | --- | --- | --- | --- |
| Empty-process RSS | ~2–8 MB | ~1–4 MB | ~2–8 MB | ~10–30 MB (GC) |
| Control of layout | Excellent (structs, arenas, indices) | Best-in-class (explicit allocators) | Excellent | Good (structs, slices); GC owns lifetime |
| Cyclic ASTs / type graphs | Awkward (arenas + IDs, not `&T`) | Fine (manual, arenas) | Fine | Natural |
| LSP SDK maturity | **Best** (`lsp-server`, `lsp-types`, `tower-lsp-server`, `async-lsp`) | Thin (zls is the example; no official SDK) | Good (`LspCpp`, `lsp-framework` C++20) | Good (`gopls` internals, `go-lsp`) |
| Parser ecosystem | tree-sitter, rowan, oxc, php-rs-parser | tree-sitter C ABI, write your own | Clang, tree-sitter, recursive descent | tree-sitter, hand-rolled |
| Cross-compile / ship | Static binary, easy | Static binary, easy | Toolchain / ABI pain (libstdc++, MSVC) | Static-ish binary, easy |
| Language stability | Stable | Still moving (0.13–0.15 churn) | Stable, slow | Stable |
| Fit with this author’s code | **native-cli-ai, chaca-scanner, aisdk already Rust** | No Zig repos | Some C / CUDA, not recent product code | POS backend; less “native CLI” muscle |
| AI / contributor density | High | Lower | High but slower iteration | High |
| Risk of fighting the language on a type checker | High (TS team tried, abandoned) | Medium | Medium | Low |

### Rust

**Use when:** greenfield server, want a single static binary, care about RAM *and* shipping, already know the language.

What is actually good today:

- [`lsp-server`](https://crates.io/crates/lsp-server) — sync, tiny, what rust-analyzer uses. Lowest overhead. You own the event loop.
- [`lsp-types`](https://crates.io/crates/lsp-types) — generated protocol types.
- [`tower-lsp-server`](https://github.com/tower-lsp-community/tower-lsp-server) — maintained fork of unmaintained `tower-lsp`. Easy trait API. Extra Tokio/locks cost.
- [`async-lsp`](https://github.com/oxalica/async-lsp) — better notification ordering than tower-lsp (`didChange` must be applied in order).

Parsers: `tree-sitter` (incremental, good enough for syntax + folding + local symbols), `rowan` (lossless CST, used by rust-analyzer and Biome — **pointer-heavy, do not store the whole workspace this way**), dedicated crates (`php-rs-parser`, oxc for JS).

Production proof that Rust LSPs can be *small*:

- [PHPantom](https://github.com/PHPantom-dev/phpantom_lsp) — PHP, Rust, claims **59 MB vs Intelephense 520 MB**, ready in &lt; 1 s vs ~85 s. WordPress-without-Composer is an explicit fallback as of 0.9.0.
- [jorgsowa/php-lsp](https://github.com/jorgsowa/php-lsp) — PHP, Rust, tokio + dashmap, tree-sitter-php.
- [Biome](https://biomejs.dev) — JS/TS/CSS/JSON lint+format LSP, not a type checker.
- [ty](https://github.com/astral-sh/ty) (Astral) — Python type checker + LSP in Rust.

Production proof that Rust LSPs can still be *huge*: rust-analyzer. Rowan CSTs + salsa incremental graph + proc macros. Matklad’s 2026 write-up: the in-memory model is sparse and “pointy”; dumping it to disk is not a good index format. You need a **compact, purpose-built on-disk format** and only keep the current crate/file hot.

**Verdict:** default choice for this project.

### Zig

**Use when:** you want allocator-visible everything, are willing to write protocol + JSON yourself, and the analysis engine is the product.

zls is the existence proof: arena allocators, little heap churn, generally low RSS. There is no first-class LSP SDK on the official list. JSON-RPC, UTF-16 offsets (LSP positions are UTF-16, not UTF-8), and cancellation are all on you.

Zig’s strength (explicit `std.mem.Allocator`) maps well to “arena per request, drop after response”. That is exactly the RAM pattern we want.

Zig’s weakness for *this* repo: standard library and package story still move; fewer parsers; fewer people (and agents) who can review a non-trivial type engine; no existing Zig in the author’s stack.

**Verdict:** best *memory model*, weakest *product velocity*. Not the first language unless the goal is “learn Zig by building an LSP”.

### C++

**Use when:** you need to reuse Clang/LLVM, or you already have a C++ frontend.

clangd is the gold-standard native LSP, and also the cautionary tale:

- Preamble/PCH for open files goes to **disk** by default (`--pch-storage=memory` is the footgun).
- Main-file ASTs live in an **LRU of 3**, not “every open tab”.
- Background index shards sit in `.cache/clangd/index/` and are still **loaded into RAM** for the whole project. Chromium: ~550 MB disk → ~2.7 GB RSS. Selective/paged loading is still a design discussion, not the default.

SDKs: [LspCpp](https://github.com/kuafuwang/LspCpp), [lsp-framework](https://github.com/leon-bckl/lsp-framework) (C++20, generated from the official meta-model). No GC, full control — and you pay with build times, UB, and shipping three toolchains.

**Verdict:** right if we were wrapping Clang. Wrong as a greenfield web/PHP server.

### Go (why TypeScript 7 picked it — and why we should not, yet)

Anders Hejlsberg / Ryan Cavanaugh (2025): they needed a **port** of an existing cyclic, GC-assuming JS compiler, not a rewrite. Rust ownership made that port intractable. Go gave native code, struct layout (memory ~halved vs tsserver), and goroutines. They were explicit: *in a type checker almost nothing can be freed until the batch is done, so Rust’s “when do we free” does not help*.

That argument applies to **porting tsserver**. It does not apply to a greenfield PHP/WordPress or HTML/CSS server. For a new engine, Rust/Zig arenas win on RSS because there is no GC heap and no 10–30 MB Go runtime floor. Also: Microsoft is already doing the TS native LSP. Competing with `tsgo` is a dead end.

**Verdict:** skip unless we later port an existing JS/PHP engine instead of writing one.

---

## 4. Architecture that actually lowers RAM

Copy clangd’s *ideas*, not clangd’s “load the whole index” default. Copy rust-glancer’s *split* between hot file and cold index.

```
                         ┌─────────────────────┐
                         │  editor (Cursor)    │
                         └─────────┬───────────┘
                                   │ stdio JSON-RPC
                         ┌─────────▼───────────┐
                         │  native-lsp binary  │
                         │  (one process)      │
                         └─────────┬───────────┘
          ┌────────────────────────┼────────────────────────┐
          ▼                        ▼                        ▼
 ┌─────────────────┐    ┌────────────────────┐    ┌─────────────────┐
 │ Open documents  │    │ Compact disk index │    │ Stubs (PHP, WP) │
 │ arena + CST     │    │ mmap, page on hit  │    │ frozen, shared  │
 │ only unsaved /  │    │ symbols, refs,     │    │ not re-parsed   │
 │ recently edited │    │ file digest        │    │ every launch    │
 └─────────────────┘    └────────────────────┘    └─────────────────┘
```

### Rules that keep RSS down

1. **Never keep the workspace CST in RAM.** Parse open files. Index the rest into a dense binary format (length-prefixed records, interned strings, integer IDs). `mmap` the file; touch pages on lookup.
2. **Arena per request.** Completion/hover allocates, responds, frees the arena. Do not retain type graphs between keystrokes except as compact cache entries.
3. **LRU the expensive artifacts.** clangd keeps 3 main-file ASTs. Do the same. Rebuild is cheaper than RSS.
4. **Do not index `vendor/`, `node_modules/`, or `wp-content/uploads` by default.** Lazy-index a package when the user actually opens or completes into it.
5. **UTF-16 positions at the edge only.** Internally use byte offsets. LSP’s UTF-16 is a tax; convert at the protocol boundary.
6. **One process, many languages** beats five Node processes. HTML + CSS + JSON + PHP in one binary removes four V8 floors even if analysis is simple.
7. **Sync protocol loop, async only for indexing.** `didChange` must be ordered. tower-lsp’s original async notifications were a correctness bug. Prefer `lsp-server` + a background thread pool for indexing, or `async-lsp`.
8. **Measure RSS, not “heap used”.** Ship `native-lsp --memory-report` and a fixture workspace (a real WP plugin + a small TS package) from day one. If idle RSS is not under **80 MB** on that fixture, the design is wrong.

### What we will not do in v1

- Full TypeScript type checker (Microsoft `tsgo`).
- rust-analyzer-style salsa over the whole project.
- Storing rowan/tree-sitter trees for every file.
- Feature-parity with Intelephense Premium on week one.

---

## 5. Existing native landscape — do not reinvent

| Gap | Who already owns it | Implication |
| --- | --- | --- |
| TypeScript semantics | `tsgo` (Go), still landing 2025–2026 | Do not write a TS type checker |
| JS/TS lint + format LSP | Biome, oxc | Reuse or ignore; don’t clone |
| PHP generic LSP | PHPantom (~59 MB), php-lsp (Rust), Phpactor (PHP), Intelephense (Node) | Commodity PHP is crowded |
| C/C++ | clangd | Out of scope |
| Zig | zls | Out of scope |
| Python | ty / ruff (Rust) | Out of scope |
| **WordPress-aware PHP** | Nobody strong. Intelephense is generic PHP. PHPantom 0.9.0 can *scan* a WP tree without Composer; it is not a WP API model | **This is the open product** |
| **One binary for HTML/CSS/JSON/PHP** replacing several vscode-* Node servers | Nobody small | Second open product |

Cursor/VS Code still spawn Node for HTML/CSS/JSON even in a PHP project. Killing those processes is a RAM win even if PHP semantics stay modest.

---

## 6. Recommendation for `native-lsp`

### Language: Rust

Not because it is fashionable. Because:

1. This author already ships native tools in Rust.
2. The LSP crate ecosystem is the only one that is both maintained and small enough (`lsp-server`).
3. tree-sitter + a custom compact index is straightforward.
4. We can match PHPantom’s “under 100 MB” envelope without Zig’s ecosystem tax or C++’s toolchain tax.
5. A greenfield engine *wants* arenas and integer IDs — which is how you write this in Rust anyway (`la-arena`, interned strings). The TS team’s “Rust is hostile to cyclic graphs” objection applies to *porting tsserver*, not to a new index.

Zig remains a valid **allocator experiment** later (or a sibling crate) if we ever want to prove a sub-20 MB syntax server. It should not block v1.

### Product: a native LSP *core*, first language PHP (WordPress-shaped)

Two layers:

1. **`native-lsp` core** — JSON-RPC, document store, tree-sitter frontends, mmap symbol index, memory budget, `--stdio`. Language-agnostic.
2. **`native-lsp php`** — PHP 7.4–8.4 parse, Composer *or* WP-style tree walk, `wp-includes` / plugin API stubs, hooks (`add_action` / `apply_filters`) as first-class symbols, `get_option` / `WP_Query` / `$wpdb` completions. This is what Intelephense does poorly on RAM and what PHPantom does not specialize.

Optional later languages (same process, extra tree-sitter grammars): HTML, CSS, JSON. That collapses the Node HTML/CSS/JSON servers. Do not take on TS semantics.

### Target numbers (fixture: a typical WP plugin + `wp-includes` stubs)

| Metric | Node today (Intelephense + html/css/json) | Native target |
| --- | --- | --- |
| Idle RSS after ready | 500–800 MB | **&lt; 80 MB** |
| Time to first completion | 10–90 s indexing | **&lt; 2 s** (stubs + open file) |
| Disk index | 45 MB+ opaque cache | Compact, restart-safe, optional |
| Processes | 3–5 × node | **1 binary** |

If we cannot beat 80 MB idle on that fixture, we failed the premise — even if the code is “native”.

### Suggested crate layout (when implementation starts)

```
native-lsp/
  crates/
    lsp-core/       # protocol loop (lsp-server), docs, cancellation
    index/          # compact on-disk symbols/refs, mmap
    parse/          # tree-sitter grammars, byte↔UTF-16
    php/            # PHP + WordPress stubs and hook index
    html/           # later
    native-lsp/     # bin: stdio server
  testdata/         # WP plugin fixture for RSS benches
```

Protocol crate: **`lsp-server` + `lsp-types`**, not Tokio-first tower-lsp. Background index = a dedicated thread pool. Tokio is optional later for watchers.

Parser v1: **tree-sitter-php** (incremental, error-tolerant, good enough for symbols and local completion). Dedicated `php-rs-parser` only if type-aware analysis needs a real AST.

---

## 7. Putting the hurdle in VRAM

The hurdle is the **workspace index** (symbols, references, later types). The question: can we keep that blob in GPU memory so system RAM stays free for the editor, `php`, and local models?

### Two different machines, two different answers

**Apple Silicon (the likely daily driver here).** There is no second pool. CPU, GPU, and Neural Engine share one DRAM. A Metal/`MTLBuffer` allocation is still process footprint; Activity Monitor counts it; memory pressure includes it. “Upload to VRAM” is a no-op — you already paid in RAM. The only GPU win on Mac is *bandwidth for dense scans*, not *hiding bytes*.

**Discrete NVIDIA (PCIe).** VRAM *is* extra. A 400 MB index on the card does not show up as CPU RSS. That part of the idea is real. The rest of the idea fights the access pattern.

### Why a GPU hash table is the wrong tool for LSP

Completion, hover, and go-to-definition are **one key (or a handful) per keystroke**. The CPU already does that in **~200–400 ns** from an mmap’d compact table (L3 / DRAM, no syscall on the hit path).

A GPU wants the opposite: millions of keys in one kernel so launch cost amortizes.

| Path | Typical cost | Fit for `textDocument/definition` |
| --- | --- | --- |
| CPU mmap lookup | 0.2–0.4 µs | Yes |
| CUDA kernel launch floor | **~5 µs** even for a null kernel (PCIe + driver, stable for years) | Already 10–25× slower before any work |
| Copy 64-byte result back over PCIe | tens of µs | Worse |
| GPU hash-table *batch* find (research tables) | ~0.3 ms per batch | Fine for “search 100k symbols”, not for one FQCN |
| NVIDIA’s own rule of thumb | pack ≥ ~1 ms of work per launch | LSP lookups are microseconds of work |

GPUs beat CPUs at *random-access throughput* (cuCollections, RAPIDS). They lose at *single-lookup latency*. An LSP is a latency service.

Pointer-rich data (CSTs, type graphs, parent pointers) cannot sit usefully in VRAM anyway. You would first flatten them into SoA / integer IDs — which is exactly the compact disk format we already want. Once it is compact, **mmap on the CPU is the faster random-access store**.

Windows WDDM makes launch latency worse than Linux/TCC. Many Cursor users are on that path.

### What VRAM *is* good for

Dense, data-parallel work where 3 ms is acceptable:

1. **Embedding matrix / ANN index** for “find code like this” (FAISS + NVIDIA cuVS / CAGRA, or a float32 sidecar like [srclight](https://github.com/slyccc/srclight) ~3 ms for 27k vectors). [Engram](https://github.com/Artemarius/Engram) does the same for MCP: tree-sitter chunks → GPU embed → HNSW, sub-3 ms. That is agent retrieval, not hover.
2. **Indexing throughput** — batch-embed changed files. Build on GPU, *search on CPU* is a documented FAISS pattern (CAGRA graph → HNSW).
3. **Workspace-wide fuzzy scan** if we ever batch “all symbols matching `wp_get_*`” as one kernel. Still optional; a SIMD CPU scan of interned names is usually enough at WP-plugin scale.

Those features fight Cursor and local LLMs for the **same** VRAM. A 428 MB embedding matrix (srclight’s 27k × 4096-d example) is a real tax next to a 7B model. The core LSP must not require a GPU.

### Decision

| Layer | Where it lives | Why |
| --- | --- | --- |
| Open-file CST / type snapshot | CPU RAM, arena, LRU | Latency, mutation, UTF-16 edge |
| Symbol / reference index (the RAM hurdle) | **Disk mmap, CPU** | 300 ns lookup, no GPU, works on every Mac |
| Optional semantic sidecar | VRAM *if* discrete GPU and user opts in; else RAM/disk | Dense GEMM/ANN; never on the completion critical path |
| Apple Silicon | Treat GPU buffers as **the same RAM** | Unified memory; do not pretend we saved RSS |

Do not put the hurdle in VRAM for v1. Keep the compact mmap index. If we later want AI-shaped `workspace/symbol` or an MCP “search this repo”, add a **feature-flagged embedding index** that can live in VRAM on NVIDIA and in unified memory on Apple — measured separately from the 80 MB idle bar.

---

## 8. Risks

- **Rebuilding Intelephense.** Feature-complete PHP analysis is years. Scope to navigation + completion + diagnostics on a budget, then deepen WP-specific intelligence.
- **rowan/salsa by default.** They optimize for incrementality and IDE fidelity, not RSS. Use them only for the *open file*.
- **Indexing vendor/wp-includes fully.** That is how Node servers reach 1 GB. Stubs + lazy type-on-demand.
- **UTF-16 / Windows paths / cancellation.** LSP edge cases eat time. Put them in `lsp-core` tests before fancy analysis.
- **PHPantom already exists.** A generic PHP clone has no reason to live. WordPress-shaped intelligence + multi-language process consolidation is the reason this repo exists.
- **VRAM as a fake RAM win.** On Apple Silicon it is the same DRAM. On NVIDIA it adds launch latency and fights local models. GPU only as an opt-in dense sidecar.

---

## 9. Sources

- LSP overview and spec 3.18: https://microsoft.github.io/language-server-protocol/
- Official SDKs: https://microsoft.github.io/language-server-protocol/implementors/sdks/
- clangd index design: https://clangd.llvm.org/design/indexing.html
- clangd Chromium RAM: https://github.com/clangd/clangd/issues/1630 (disk ~550 MB → RSS ~2.7 GB)
- TypeScript → Go: https://github.com/microsoft/typescript-go/discussions/411
- rust-glancer / low-RAM LSP: https://matklad.github.io/2026/08/21/rust-glancer.html
- PHPantom: https://github.com/PHPantom-dev/phpantom_lsp
- php-lsp (Rust): https://github.com/jorgsowa/php-lsp
- tower-lsp-server: https://github.com/tower-lsp-community/tower-lsp-server
- async-lsp vs tower-lsp notification ordering: https://github.com/oxalica/async-lsp
- Intelephense OOM / maxMemory: https://github.com/bmewburn/vscode-intelephense/issues/590
- Node empty-process RSS ~30–50 MB: widely reproduced; V8 new-space also inflates RSS under load
- Apple Silicon unified memory (no separate VRAM): https://www.macinternals.app/en/blog/apple-gpu-and-metal
- CUDA kernel launch floor ~5 µs: NVIDIA forums, long-standing PCIe lower bound
- GPU hash maps are throughput tools: https://developer.nvidia.com/blog/maximizing-performance-with-massively-parallel-hash-maps-on-gpus/
- mmap CPU lookup ~350 ns p50: maph / similar compact indexes
- GPU code-search sidecars: https://github.com/Artemarius/Engram, https://github.com/slyccc/srclight
- FAISS + NVIDIA cuVS (build on GPU, search on CPU is supported): https://engineering.fb.com/2025/05/08/data-infrastructure/accelerating-gpu-indexes-in-faiss-with-nvidia-cuvs/

---

## 10. Decision

| Decision | Choice |
| --- | --- |
| Native language | **Rust** |
| Not now | Zig (keep as allocator experiment), C++ (no Clang to wrap), Go (no tsserver to port) |
| Protocol stack | `lsp-server` + `lsp-types`, sync main loop |
| RAM strategy | open-file CST + mmap compact index + per-request arenas |
| VRAM | **Not for the symbol index.** Optional later embedding sidecar, NVIDIA opt-in; Apple Silicon counts as RAM |
| First language | PHP with WordPress stubs/hooks |
| Success bar | &lt; 80 MB idle RSS on a WP plugin fixture, &lt; 2 s to first completion |
| Non-goal | TypeScript type checker, Intelephense-complete PHP in v1, GPU required for core LSP |

Implementation should not start until this direction is accepted. The next commit after that is a hello-world `initialize` / `shutdown` server that prints RSS, not a parser.
