# vowl-wasm

[![crates.io](https://img.shields.io/crates/v/vowl-wasm.svg)](https://crates.io/crates/vowl-wasm)
[![docs.rs](https://docs.rs/vowl-wasm/badge.svg)](https://docs.rs/vowl-wasm)
[![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)

An ontology visualisation engine in Rust, compiled to WebAssembly.

It turns an OWL ontology into something a browser can draw: parse the ontology,
build a typed graph from it, settle that graph with a force-directed layout, and
hand the result to JavaScript — either as structured data, or as a pointer into
WASM linear memory that a rendering worker reads without copying.

<!-- lands with ontomark step 5 -->
This crate renders; it does not own any ontology format. With the `ngg1`
feature, `vowl_wasm::ngg1` and `NggExplorer` read NGG1 binary graph tiers through
a re-export of the [`ontomark-ngg1`](https://crates.io/crates/ontomark-ngg1) crate from
[ontomark](https://github.com/DreamLab-AI/ontomark). ontomark is the single owner of the
NGG1 specification, constants and codec; this crate holds no format constants of
its own. The JavaScript API is unchanged.

## Attribution

This is a **clean-room Rust reimplementation** of the **VOWL** visual notation
defined by [VisualDataWeb/WebVOWL](https://github.com/VisualDataWeb/WebVOWL)
(MIT, © 2014–2019 Vincent Link, Steffen Lohmann, Eduard Marbach, Stefan Negru,
Vitalis Wiens). WebVOWL is the origin of the visual notation and of the ontology
JSON shape this crate reads; none of its JavaScript, fixtures, or assets are
vendored here. Both licence notices are carried in [`LICENSE`](./LICENSE).

This crate is not affiliated with or endorsed by the WebVOWL project or TIB.

## Install

```toml
[dependencies]
vowl-wasm = "0.1"
```

From JavaScript, via the published bundle:

```bash
npm install @dreamlab-ai/vowl-wasm
```

## Rust usage

```rust
use vowl_wasm::bindings::WebVowl;

let mut vowl = WebVowl::new();

vowl.load_ontology(r#"{
    "class": [
        { "id": "c1", "label": "Person",       "type": "owl:Class" },
        { "id": "c2", "label": "Organisation", "type": "owl:Class" }
    ],
    "property": [
        { "id": "p1", "label": "worksFor", "type": "owl:ObjectProperty",
          "domain": "c1", "range": "c2" }
    ]
}"#).expect("valid ontology");

vowl.init_simulation().expect("graph is loaded");
vowl.run_simulation(120).expect("graph is loaded");

assert_eq!(vowl.get_node_count(), 2);
```

## JavaScript usage

```js
import init, { WebVowl } from '@dreamlab-ai/vowl-wasm';

await init();

const vowl = new WebVowl();
vowl.loadOntology(ontologyJson);
vowl.initSimulation();
vowl.runSimulation(300);

console.log(vowl.getNodeCount(), vowl.getStatistics(), vowl.getMetadata());
```

### Reading positions back

`getNodePositions()` returns a `Float32Array` of interleaved `x, y` in node
index order. That index order is **stable for the lifetime of a loaded graph** —
ticking, pinning, filtering and folding never reorder or remove nodes — so map
index to id once and then read only floats per frame:

```js
const ids = vowl.getNodeIds();        // once, after loadOntology

function frame() {
  vowl.tick();
  const xy = vowl.getNodePositions(); // ids[i] <-> xy[2i], xy[2i + 1]
  for (let i = 0; i < ids.length; i++) {
    moveNode(ids[i], xy[2 * i], xy[2 * i + 1]);
  }
}
```

To reuse one buffer instead of allocating per frame, use
`writePositionsInto(buf)`, which fills a caller-owned `Float32Array` and returns
the number of floats written:

```js
const xy = new Float32Array(vowl.getNodeCount() * 2);
vowl.tick();
draw(xy.subarray(0, vowl.writePositionsInto(xy)));
```

Labels, types and edges still come from `getGraphData()`, which stays behind the
`debug-serde` feature.

For a rendering worker, `NggExplorer` avoids per-frame serialisation entirely:

```js
import init, { NggExplorer } from '@dreamlab-ai/vowl-wasm';

const wasm = await init();
const explorer = new NggExplorer();
explorer.loadCsr(tierBytes);

// A zero-copy view over the live position buffer.
const positions = new Float32Array(
  wasm.memory.buffer, explorer.positionsPtr(), explorer.positionsLen()
);

while (!explorer.isFinished()) {
  explorer.tick();
  draw(positions); // no copy, no JSON
}
```

The view is invalidated by the next `loadCsr` and by any WASM memory growth —
re-read `positionsPtr()` after either.

## Public API

This is the **frozen 0.1.0 surface**. Names and signatures will not change
without a major version bump; later 0.1.x releases only add to it.

### `WebVowl` — always available

| JavaScript | Purpose |
|---|---|
| `new WebVowl()` | Construct an empty instance |
| `loadOntology(json)` | Parse the ontology and build the graph |
| `initSimulation()` | Seed the force simulation |
| `runSimulation(n)` | Advance `n` steps |
| `tick()` | Advance one step |
| `isFinished()` | Has the layout settled? |
| `getAlpha()` | Current annealing temperature |
| `setCenter(x, y)` | Move the layout centre |
| `setLinkDistance(d)` | Spring rest length |
| `setChargeStrength(s)` | Repulsion strength |
| `getNodeCount()` / `getEdgeCount()` | Graph size |
| `getNodeIds()` | Node ids, in index order |
| `getNodePositions()` | Live `x, y` per node as a `Float32Array` |
| `writePositionsInto(buf)` | Copy those positions into a caller-owned buffer |
| `getStatistics()` | Degree, connected components, OWL2 metrics |
| `getMetadata()` | Ontology header: IRI, version, title, description |

Filtering, folding and pinning are also always available: `filterByDegree`,
`filterHierarchy`, `toggleDatatypes`, `toggleSetOperators`, `resetVisibility`,
`getNodeDegree`, `pinNode`, `unpinNode`, `isPinned`, `resetPins`,
`getPinnedCount`.

### Feature-gated

| JavaScript | Feature |
|---|---|
| `new NggExplorer()` — `loadCsr`, `tick`, `isFinished`, `getAlpha`, `nodeCount`, `edgeCount`, `positionsPtr`, `positionsLen`, `writePositionsInto`, `setParam`, `reheat` | `ngg1` |
| `checkNodeClick(origin, direction)` | `interaction` |
| `getGraphData()` | `debug-serde` |

## Features

Every optional feature is **off by default**. The general-purpose engine —
OWL/JSON parsing, the graph model, force layout (Barnes-Hut quadtree and the CSR
simulation), pinning and statistics — is always compiled.

| Feature | Default | What it adds |
|---|---|---|
| `ngg1` | off | The NGG1 reader (`vowl_wasm::ngg1`) and the `NggExplorer` zero-copy binding |
| `interaction` | off | Ray-sphere hit testing for click and selection |
| `debug-serde` | off | Legacy full-graph serde export, for diagnostics |
| `parallel` | off | Data-parallel ontology loading via rayon. **Native only** |
| `simd` | off | WASM SIMD128 force accumulation; needs the `simd128` target feature |

Each feature compiles standalone, and the combinations are checked in CI.

**The published npm bundle is built with `--features ngg1,interaction`.**

<!-- lands with ontomark step 5 -->
The `markdown-ontology` feature, with its Markdown parser and OWL 2 validator,
was removed. Parsing Markdown vaults is `ontomark-parse`'s job.
`parseMarkdownOntology` and `validateOWL2` were not in the 0.1.2 published bundle
(0.1.0 and 0.1.1 shipped them); they are gone from the crate as well.

Version 0.2.0 also removes the Rust-only `ontology::Domain` and
`ontology::OntologyBlock` types. Class metadata accepts caller-supplied domain
strings, including domains outside the estate, or no domain. The renderer holds
no domain registry or namespace table.

### Reading and writing NGG1 in JavaScript

With `ngg1` enabled, `decodeNgg1(bytes)` returns an owned `NggGraph`.
`ids()` and `degrees()` return `Uint32Array`; `positions()` returns packed
`Float32Array` coordinates; `domains()` and `categories()` return `Uint16Array`;
`flags()` and `edgeType()` return `Uint8Array`. `labels()` and `iris()` return
string arrays. `rowPtr()` and `colIdx()` return CSR `Uint32Array` snapshots.
All arrays are independent copies and remain usable after `graph.free()`.

```javascript
import init, { decodeNgg1, encodeNgg1, ngg1Constants } from '@dreamlab-ai/vowl-wasm';
await init();
const graph = decodeNgg1(bytes); // Uint8Array containing a tier
const fields = [
  graph.positions(), graph.ids(), graph.domains(), graph.categories(),
  graph.flags(), graph.degrees(), graph.labels(), graph.iris(),
  graph.rowPtr(), graph.colIdx(), graph.edgeType(),
];
graph.free();
const encoded = encodeNgg1(...fields); // standalone Uint8Array
const { CATEGORY_NONE, FLAG_META } = ngg1Constants();
```

For synthetic graphs, supply seed positions and metadata in the same local
node order. Every node field and string array must match `ids.length`, positions
must have twice that length, and CSR targets must be valid local indices.
Malformed input throws an `NGG1: ...` string. The owned decoder rejects invalid
UTF-8; unknown taxonomy ids, flag bits and edge codes are preserved. The existing
`NggExplorer` remains the zero-copy physics interface for animation.

**Bundle size.** Measured with the published-bundle build,
`wasm-pack build --release --target web --features ngg1,interaction`
(wasm-pack 0.15, wasm-bindgen 0.2.128, `wasm-opt` off as in `Cargo.toml`), on
2026-10-11:

| Tree | `pkg/vowl_wasm_bg.wasm` | `gzip -9` |
|---|---|---|
| 0.1.2 source (hand-written NGG1 reader) | 232,764 bytes | 91,418 bytes |
| 0.2.0 (shared codec, owned decode/encode bindings) | 264,158 bytes | 101,069 bytes |

The 0.1.2 release notes record 232,052 bytes (90,675 gzipped); the difference is
toolchain drift, not a source change.

The 0.2.0 browser decoder, encoder and constants add 31,394 bytes (+13.49%) raw
over the 0.1.2 source baseline. No reasoner
(`whelk-el`), RDF, YAML or regex crate may enter the bundle's dependency graph:
`tests/ontomark_ngg1_migration.rs` walks the normal dependencies of
`--features ngg1,interaction` (dev-dependencies such as `criterion` do not ship).
With `VOWL_BUNDLE_GATE=1` after a `wasm-pack build` (CI does this), it also
requires `pkg/vowl_wasm_bg.wasm` to be a real WebAssembly module below 280,000
bytes, with the recorded 0.2.0 JavaScript API in `pkg/vowl_wasm.d.ts`.

`parallel`, `simd` and `debug-serde` are likewise off in the published bundle;
build from source to enable them.

## Building

```bash
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings

# The published bundle
wasm-pack build --target web --release --scope dreamlab-ai \
  --features ngg1,interaction
node --test tests/ngg1_wasm.test.mjs
```

`pkg/` is a build artefact and is not committed. CI uploads it as a workflow
artifact, and tagged releases attach it as `vowl-wasm-<version>-pkg.tgz` and
publish that same tarball to npm as `@dreamlab-ai/vowl-wasm` through npm
trusted publishing (OIDC from `release.yml`; no token is stored anywhere and
provenance attestations are generated automatically). To republish an existing
tag, run the Release workflow with the tag as its input; it skips versions the
registry already has.

MSRV: **1.85**.

## Migrating from `webvowl-wasm` / `@dreamlab-ai/webvowl-wasm`

This crate replaces three divergent in-tree copies of `webvowl-wasm`. Migrating:

1. **Package name.** `webvowl-wasm` → `@dreamlab-ai/vowl-wasm`. Replace any
   `file:` path dependency with a real semver pin.

2. **Generated file names change**, because the crate's lib name changed from
   `webvowl_wasm` to `vowl_wasm`:

   | Before | After |
   |---|---|
   | `webvowl_wasm.js` | `vowl_wasm.js` |
   | `webvowl_wasm.d.ts` | `vowl_wasm.d.ts` |
   | `webvowl_wasm_bg.wasm` | `vowl_wasm_bg.wasm` |

   Anything that references those files by path — a bundler `optimizeDeps`
   entry, a hand-written `init(url)` call, a CDN URL, a CSP hash — needs
   updating. Importing the package by name is unaffected.

3. **Version resets to 0.1.0.** Neither of the old copies (1.0.0 and 0.3.4)
   described this API, so the numbering starts over. There is no implied
   compatibility with either.

4. **Feature gates.** `NggExplorer` and `checkNodeClick`
   were unconditional before and are now behind `ngg1` and `interaction`
   <!-- lands with ontomark step 5 --> (the Markdown parser has since been removed; see [Features](#features)).
   The published bundle enables both; a consumer building from source must ask
   for them.

5. **Layout defaults changed.** `LayoutConfig::default()` now carries
   general-purpose force constants rather than values tuned for one
   1,155-node ontology. The old numbers are available as
   `LayoutConfig::large_graph()`. This affects `WebVowl` only — `NggExplorer`
   and `CsrSimulation` have their own parameters and are unchanged.

## Licence

MIT. See [`LICENSE`](./LICENSE), which carries both this project's notice and
the upstream WebVOWL notice.
