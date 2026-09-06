# Changelog

All notable changes to this project are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/DreamLab-AI/vowl-wasm/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/DreamLab-AI/vowl-wasm/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/DreamLab-AI/vowl-wasm/releases/tag/v0.1.0
