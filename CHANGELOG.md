# Changelog

All notable changes to this project are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- NGG1 is now a re-export of the [`ontomark-ngg1`](https://crates.io/crates/ontomark-ngg1)
  crate (0.1.0). `src/ngg1.rs` is `pub use ontomark_ngg1::*;` and carries no magic
  number, stride, offset or flag constant of its own. The `vowl_wasm::ngg1` path,
  `Ngg1::parse` and the `WebVowl` and `NggExplorer` JavaScript API
  (`pkg/vowl_wasm.d.ts`) are unchanged. The published bundle is built with
  `--features ngg1,interaction`; no reasoner, RDF, YAML or regex crate enters the
  dependency graph.

### Breaking

- **NGG1 malformed buffers now return an error instead of panicking.** The parser
  additionally rejects a `row_ptr` that does not start at 0, decreases or does not
  end at the edge count, an edge target that is not a node index, string offsets
  out of order or past the blob, and a section offset inside the header. For
  JavaScript callers, `NggExplorer.loadCsr(bytes)` now throws a string error
  (`NGG1: ...`) for such buffers, where 0.1.2 accepted them and could trap
  (abort, since `panic = "abort"`) later in `tick` or when reading positions.
  Buffers written by the NGG1 writer are unaffected.
- **Removed JavaScript methods `WebVowl.parseMarkdownOntology` and
  `WebVowl.validateOWL2`**, with the Rust items behind them (below). Neither was in
  the published npm bundle.

### Removed

- The `markdown-ontology` feature and everything behind it: `ontology::markdown_parser`,
  `ontology::loader`, `ontology::owl2_validator`, the `parseMarkdownOntology` and
  `validateOWL2` bindings, and the `regex` dependency. Markdown vault parsing belongs
  to `ontomark-parse`. The `Domain` enum and `OntologyBlock` remain until the
  taxonomy step removes them.

## [0.1.2] - 2026-09-06

### Changed

- **The published npm bundle no longer includes `markdown-ontology`**, and is
  built with `--features ngg1,interaction`. That feature was the crate's only
  consumer of a regex engine, and `regex` + `aho-corasick` + `memchr` were 68%
  of 0.1.0's compiled code — the largest single thing in the download, for a
  parser neither in-estate consumer calls. The bundle goes from 1,114,091 to
  **232,052 bytes** (−79.2%); gzip 379,421 to **90,675** (−76.1%), brotli
  278,639 to **77,823** (−72.1%). Measured against 0.1.0, that is −84.3% raw and
  −81.2% gzip.

  `parseMarkdownOntology` and `validateOWL2` are therefore absent from the
  published bundle. Nothing about the crate changed: build with
  `--features ngg1,markdown-ontology,interaction` to get them back.

- `regex` is now an optional dependency, pulled in only by `markdown-ontology`.
  The three grammars `ontology::owl2_validator` used to share with it — the
  RFC 3986 URI scheme, an ASCII PascalCase identifier, and the `AI-0850` term-id
  format — are hand-rolled ASCII matchers, so a default build links no regex
  engine at all. Behaviour is unchanged but for one deliberate tightening: the
  `regex` crate's `\d` is Unicode-aware and had been accepting non-ASCII digits
  in a term id, which the format never intended.

## [0.1.1] - 2026-09-06

Additive only: every 0.1.0 name and signature is unchanged.

### Added

- `WebVowl.getNodePositions()` — live node positions as a `Float32Array` of
  interleaved `x, y` in node index order. Always available, so reading
  coordinates after `tick()` no longer needs the `debug-serde`-gated
  `getGraphData()`, which is absent from the published bundle.
- `WebVowl.writePositionsInto(buf)` — writes those same positions into a
  caller-owned `Float32Array` and returns the number of floats written, for
  callers that would rather reuse one buffer than allocate per frame. Mirrors
  `NggExplorer.writePositionsInto` on the `ngg1` path.
- `WebVowl.getNodeIds()` — node ids in that same index order, so a consumer can
  build the index-to-id map once and then read only floats per frame.
- Documented the index-order guarantee: node index order is stable for the
  lifetime of a loaded graph — ticking, pinning, filtering and folding never
  reorder or remove nodes — and its length always matches `getNodeCount()`.

### Changed

- The release profile is tuned for the published `.wasm`: `opt-level = "z"`,
  `strip = true` and `panic = "abort"` join the existing `lto` and
  `codegen-units = 1`. The estate bundle drops from **1,477,462 to 1,114,091
  bytes** (−24.6%); gzip from 481,493 to 379,414 (−21.2%) and brotli from
  336,704 to 279,066 (−17.1%). Most of the raw saving is the 190,177-byte `name`
  section that `strip` removes; the rest is `opt-level = "z"` over `3`.
- `wasm-opt` stays off, now deliberately and with the measurement recorded in
  `Cargo.toml`: on top of an `opt-level = "z"` + LTO build, `-Oz --strip-debug`
  shrinks the raw module by a further 81 KB but makes both the gzip and the
  brotli transfer *larger*.

### Fixed

- The published `package.json` no longer claims `"sideEffects": ["./snippets/*"]`
  for a bundle that ships no `snippets/` directory. It is now `false`, which is
  true of this bundle and lets a bundler tree-shake the glue.

### Internal

- The `bindings` unit tests now run under `cargo test` as well as
  `wasm-pack test`. A bare `#[wasm_bindgen_test]` expands to nothing off
  `wasm32`, so the existing two had never run in CI.

## [0.1.0] - 2026-09-06

### Added

- Initial release: one canonical crate replacing three divergent in-estate
  copies of `webvowl-wasm`. OWL/JSON parsing, the typed VOWL graph model,
  force-directed layout (Barnes-Hut quadtree and a CSR simulation), OWL2
  validation, pinning, filtering and statistics, behind the frozen `WebVowl`
  JavaScript surface, plus the zero-copy `NggExplorer` worker binding under the
  `ngg1` feature.

[Unreleased]: https://github.com/DreamLab-AI/vowl-wasm/compare/v0.1.2...HEAD
[0.1.2]: https://github.com/DreamLab-AI/vowl-wasm/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/DreamLab-AI/vowl-wasm/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/DreamLab-AI/vowl-wasm/releases/tag/v0.1.0
