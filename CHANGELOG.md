# Changelog

All notable changes to this project are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- Release workflow: the check after `npm publish` now polls the registry with
  the cache bypassed for up to about three minutes, and fails only if the
  version is still not served or its `dist.integrity` differs from the tarball
  attached to the GitHub release. The v0.3.0 run was marked failed because a
  single immediate `npm view` met registry propagation lag (E404) after the
  publish had been accepted.

## [0.3.0] - 2026-11-23

Released to GitHub (browser tarball and notices, with `.sha256` files) and to
npm as `@dreamlab-ai/vowl-wasm@0.3.0`, the first version published to npm
through trusted publishing, with a provenance attestation. The npm tarball is
byte-identical to the release asset `vowl-wasm-0.3.0-pkg.tgz` (sha256
`d2af7e506b3ca5c1d1071aeef3c521ef90a41c699f2d82d27292615a3e86d305`). Not
published to crates.io. The Release run reports failure only because its
post-publish check ran before the registry served the new version (see
Unreleased).

A minor (breaking) release: the layout fix below changes the force sign and
the centring semantics that direct Rust callers of `vowl_wasm::layout` see.
The JavaScript `WebVowl` and `NggExplorer` surfaces keep their signatures; the
frozen export surface only gains `NggExplorer.loadArrays`.

### Breaking changes for Rust callers

- `layout::force::calculate_repulsion`, `calculate_batch_repulsion`, the
  `layout::simd` repulsion and leaf kernels and the leaf path of
  `QuadTree::calculate_force` now return a repulsive force for a **negative**
  strength (d3-force convention, as `LayoutConfig::charge_strength` already
  documented). A caller that passed a positive strength to get repulsion must
  negate it.
- `LayoutConfig::center_strength` now means d3-force `forceCenter` strength (a
  centroid translation per tick), not a per-node spring towards the centre.
  The default value is unchanged; a caller that tuned it for the old spring
  will see much weaker centring of individual nodes.
- `WebVowl.loadOntology` keeps the simulation settings made before it instead
  of resetting them to defaults.

### Added

- `NggExplorer.loadArrays(positions, edgeA, edgeB, edgeType)`: load a plain graph
  into the force layout without encoding NGG1 first. Node `i` is
  `positions[2i], positions[2i + 1]` and the order is stable until the next load.
  Input is validated before it reaches `CsrSimulation::load_arrays`, so an odd
  `positions` length, a non-finite position, non-parallel edge arrays or an
  out-of-range endpoint throw an error and leave the loaded graph intact instead
  of aborting the WASM instance. Self-loops are accepted but exert no force;
  duplicate edges pull once per copy. Additive; recorded in the frozen export
  surface.
- `ForceSimulation::reset`: restart alpha and the iteration count while keeping
  the configuration.
- Seven owl2vowl ontologies (FOAF, SIOC, GoodRelations, MUTO, OntoViBe,
  PersonasOnto and the WebVOWL benchmark), merged into the engine contract, as
  test fixtures under `tests/fixtures/ontologies/`, and a convergence test
  (`tests/webvowl_layout_test.rs`) that drives them through the `WebVowl` API.

### Changed

- The many-body kernels follow the d3-force sign convention that
  `LayoutConfig::charge_strength` documents: a **negative strength repels**.
  `layout::force::calculate_repulsion` and `calculate_batch_repulsion`, the
  `layout::simd` repulsion and leaf kernels, and the leaf path of
  `QuadTree::calculate_force` previously returned the opposite direction (only
  the Barnes-Hut far field already repelled). A direct caller that passed a
  positive strength to get repulsion must now pass a negative one. Their
  magnitude is also held at `|strength|` below a distance of one unit. Callers
  of `WebVowl.setChargeStrength` with a negative value (the default `-30`, and
  WasmVOWL's `-300`) need no change.
- `LayoutConfig::center_strength` is now d3-force's `forceCenter` strength
  (the fraction of the centroid's offset removed per tick, applied as a
  translation) instead of a per-node spring towards the centre. The default
  value is unchanged.
- Packaging: the crate's `exclude` list now omits `tests/fixtures/ontologies/`
  and the test that embeds it (`tests/webvowl_layout_test.rs`). The fixtures
  keep their own third-party licences (PersonasOnto is CC BY-SA 3.0; SIOC
  states none) and do not ship in the MIT-licensed crate. The npm package was
  already limited to the wasm-pack `files` allowlist plus the notices.

### Fixed

- The `WebVowl` force simulation (`ForceSimulation`) no longer diverges. On the
  owl2vowl test ontologies the 0.2.3 layout reached peak coordinates of 1e7
  (MUTO) to 3e30 (GoodRelations); they now peak at 317 to 390 (the nodes are
  seeded on a circle of radius 300) and motion settles in 19 to 48 ticks.
  Four causes, all fixed:
  - Springs were applied per outgoing neighbour, on the source only, at full
    stiffness per edge, and scaled by alpha twice. A hub with *k* out-edges
    (17 in FOAF, 28 in GoodRelations) carried stiffness *k*, beyond what the
    explicit integrator can hold (about 5), so it grew roughly tenfold per
    tick. Each edge now acts once on both ends with d3-force's `forceLink`
    default: stiffness `link_strength / min(degree(source), degree(target))`,
    shared by degree, so no node's total stiffness exceeds `link_strength` and
    parallel edges no longer stack. Alpha is applied once, in the integrator.
  - The many-body force attracted at short range: for a negative
    `charge_strength`, every path except the Barnes-Hut far field pushed
    towards the other node (see Changed).
  - The many-body magnitude is softened below a distance of one unit
    (d3-force's `distanceMin`), so a near-coincident pair no longer throws a
    node hundreds of units in one tick.
  - The centre force pulled every node towards the centre at full strength,
    which crushed the layout to a fraction of one link length. It is now a
    centroid translation (see Changed). `LayoutConfig::large_graph` keeps
    `0.001`, which now means the centroid drifts almost freely.
- `WebVowl.loadOntology` keeps the simulation settings (`setLinkDistance`,
  `setChargeStrength`, `setCenter`) made before it; it used to replace the
  simulation with a default one. It still restarts alpha and clears pins.
- Known limitation: under the centroid-only centring, nodes in small
  disconnected components (FOAF has 8 isolated nodes, OntoViBe 13) stay near
  the seed circle (radius 300) while the largest component settles to a
  radius of 36 to 102. A weak per-node gravity or a seed radius scaled to the
  graph would pull them in.
- A per-tick step cap (twice the larger of `link_distance` and `seed_radius`)
  and a non-finite-velocity reset guard the integrator. Neither is reached on
  the test ontologies.

## [0.2.3] - 2026-11-03

GitHub release only (browser tarball and notices); not published to npm or
crates.io.

### Fixed

- The legacy `QuadTree` keeps coincident nodes in bounded-depth leaves instead
  of subdividing without limit when more nodes than a leaf holds share a
  position.
- `CsrSimulation` numeric setters (`set_charge`, `set_link_distance`,
  `set_link_strength`, `set_damping`, `set_center_gravity`, `set_theta` and the
  rest) ignore non-finite values instead of storing them.
- `WebVowl.loadOntology` clears pins and restarts the simulation, so a reload
  no longer inherits the previous graph's pinned nodes and cooled alpha.

### Changed

- The release workflow reuses a checksum-verified existing GitHub release
  bundle when re-run for a registry retry, rather than rebuilding or replacing
  the immutable asset.

## [0.2.2] - 2026-10-30

### Added

- Node pinning for drag interaction: `CsrSimulation::pin`/`unpin`/`is_pinned`
  and the bindings `NggExplorer.pinNode(index, x, y)` and
  `NggExplorer.unpinNode(index)`. A pinned node holds its position with zero
  velocity and still exerts forces, so its neighbours follow it. Out-of-scope
  indices and non-finite coordinates return `false` instead of panicking, and
  every load clears the pins. Additive; recorded in the frozen export surface.

## [0.2.1] - 2026-10-30

### Fixed

- `CsrSimulation` repulsion is softened below a quarter of `link_distance`, and
  each node's per-tick displacement is capped at twice `link_distance`. A
  near-coincident seed pair was previously thrown thousands of units on the
  first tick (`charge / d²` is unbounded as `d → 0`). Both bounds derive from
  `link_distance`; the exported surface is unchanged.

### Added

- Barnes-Hut quadtree repulsion for `CsrSimulation` (default `theta = 0.9`),
  with `setTheta`, and `reheat` on `ForceSimulation` and `WebVowl` (ER-20).

## [0.2.0] - 2026-10-11

### Added

- JavaScript `decodeNgg1(bytes)` returns an owned `NggGraph` with typed snapshots
  of every node field, UTF-8 label/IRI and CSR array. `encodeNgg1(...)` writes
  synthetic and focus tiers through `ontomark-ngg1`, validating field lengths
  and CSR. `ngg1Constants()` exposes the owner's flags, sentinels and edge codes.
  The existing `WebVowl` and `NggExplorer` exports retain their signatures.
- Native and real WASM tests verify byte-exact golden encoding, arbitrary
  taxonomy ids, Unicode strings, unknown codes, empty graphs and malformed input.

### Changed

<!-- lands with ontomark step 5 -->
- NGG1 is now a re-export of the [`ontomark-ngg1`](https://crates.io/crates/ontomark-ngg1)
  crate (0.1.0). `src/ngg1.rs` is `pub use ontomark_ngg1::*;` and carries no magic
  number, stride, offset or flag constant of its own. The `vowl_wasm::ngg1` path,
  `Ngg1::parse` and existing `WebVowl` and `NggExplorer` JavaScript methods
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
  the 0.1.2 published npm bundle (0.1.0 and 0.1.1 shipped them).
- **Removed Rust types `ontology::Domain` and `ontology::OntologyBlock`.** They
  belonged to the deleted Markdown parser. Class metadata still accepts any
  caller-supplied domain string, or none; binary taxonomy ids are producer data.

### Removed

- The `markdown-ontology` feature and what it gated: `ontology::markdown_parser`,
  `ontology::loader`, the `parseMarkdownOntology` and `validateOWL2` bindings, and the
  `regex` dependency. `ontology::owl2_validator` was not behind the feature (its
  `pub mod` was unconditional in 0.1.2, only the `validateOWL2` binding was gated);
  it is removed with the rest under decision D8. Markdown vault parsing belongs
  to `ontomark-parse`. The six-domain registry, its estate namespace mapping and
  `OntologyBlock` are removed with the taxonomy migration.

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
  0.1.2 published bundle (the bindings were behind the feature; the
  `ontology::owl2_validator` module itself was compiled in regardless). Nothing about the crate changed: build with
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

[Unreleased]: https://github.com/DreamLab-AI/vowl-wasm/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/DreamLab-AI/vowl-wasm/compare/v0.2.3...v0.3.0
[0.2.3]: https://github.com/DreamLab-AI/vowl-wasm/compare/v0.2.2...v0.2.3
[0.2.2]: https://github.com/DreamLab-AI/vowl-wasm/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/DreamLab-AI/vowl-wasm/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/DreamLab-AI/vowl-wasm/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/DreamLab-AI/vowl-wasm/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/DreamLab-AI/vowl-wasm/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/DreamLab-AI/vowl-wasm/releases/tag/v0.1.0
