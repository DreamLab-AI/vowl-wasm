//! `vowl-wasm` — an ontology visualisation engine in Rust, compiled to WebAssembly.
//!
//! This crate turns an OWL ontology into something a browser can draw: it parses
//! the ontology, builds a typed graph out of it, settles that graph with a
//! force-directed layout, and hands the result to JavaScript. It is a clean-room
//! Rust reimplementation of the **VOWL** visual notation defined by
//! [VisualDataWeb/WebVOWL](https://github.com/VisualDataWeb/WebVOWL) (MIT,
//! © 2014–2019 Vincent Link, Steffen Lohmann, Eduard Marbach, Stefan Negru,
//! Vitalis Wiens). No upstream JavaScript, fixtures, or assets are vendored here;
//! only the notation and the on-the-wire JSON shape are shared.
//!
//! # The two entry points
//!
//! [`WebVowl`](bindings::WebVowl) is the general-purpose surface: load an
//! ontology, run the simulation, read the graph back. It is always available.
//!
//! [`NggExplorer`](bindings::explorer::NggExplorer) is a narrower, allocation-free
//! surface for a rendering worker: it loads a pre-baked binary tier and exposes
//! the live position buffer as a pointer into WASM linear memory, so JavaScript
//! can build a zero-copy `Float32Array` over it and never pay a per-frame
//! serialisation cost. It requires the `ngg1` feature.
//!
//! # Example
//!
//! Parse a small ontology, settle it, and read the result. This is the same API
//! JavaScript sees, minus the `camelCase` renaming applied at the WASM boundary.
//!
//! ```
//! use vowl_wasm::bindings::WebVowl;
//!
//! let mut vowl = WebVowl::new();
//!
//! // WebVOWL-shaped ontology JSON: classes and the properties between them.
//! vowl.load_ontology(r#"{
//!     "class": [
//!         { "id": "c1", "label": "Person",       "type": "owl:Class" },
//!         { "id": "c2", "label": "Organisation", "type": "owl:Class" }
//!     ],
//!     "property": [
//!         { "id": "p1", "label": "worksFor", "type": "owl:ObjectProperty",
//!           "domain": "c1", "range": "c2" }
//!     ]
//! }"#).expect("valid ontology");
//!
//! assert_eq!(vowl.get_node_count(), 2);
//!
//! // Settle the layout: seed the simulation, then anneal it.
//! vowl.init_simulation().expect("graph is loaded");
//! vowl.run_simulation(120).expect("graph is loaded");
//!
//! // Alpha decays as the layout cools.
//! assert!(vowl.get_alpha() < 1.0);
//! ```
//!
//! # The JavaScript API
//!
//! Every method below is exported to JavaScript under the `js_name` shown. This
//! is the **frozen 0.1.0 surface**: names and signatures will not change without
//! a major version bump. Later 0.1.x releases only add to it — `getNodeIds`,
//! `getNodePositions` and `writePositionsInto` arrived in 0.1.1.
//!
//! ## Reading positions
//!
//! [`get_node_positions`](bindings::WebVowl::get_node_positions) is the
//! always-available way to get coordinates out after a
//! [`tick`](bindings::WebVowl::tick). Node **index order is stable for the
//! lifetime of a loaded graph** — ticking, pinning, filtering and folding never
//! reorder or remove nodes — so pair it once with
//! [`get_node_ids`](bindings::WebVowl::get_node_ids) and then read only floats
//! per frame:
//!
//! ```javascript
//! const ids = vowl.getNodeIds();          // once, after loadOntology
//! vowl.tick();
//! const xy = vowl.getNodePositions();     // ids[i] <-> xy[2i], xy[2i + 1]
//! ```
//!
//! `getGraphData()` remains the way to read labels, types and edges, and stays
//! behind `debug-serde`.
//!
//! ## `WebVowl` — always available
//!
//! | JavaScript | Rust | Purpose |
//! |---|---|---|
//! | `new WebVowl()` | [`WebVowl::new`](bindings::WebVowl::new) | Construct an empty instance |
//! | `loadOntology(json)` | [`load_ontology`](bindings::WebVowl::load_ontology) | Parse and build the graph |
//! | `initSimulation()` | [`init_simulation`](bindings::WebVowl::init_simulation) | Seed the force simulation |
//! | `runSimulation(n)` | [`run_simulation`](bindings::WebVowl::run_simulation) | Advance `n` steps |
//! | `tick()` | [`tick`](bindings::WebVowl::tick) | Advance one step |
//! | `isFinished()` | [`is_finished`](bindings::WebVowl::is_finished) | Has the layout settled? |
//! | `getAlpha()` | [`get_alpha`](bindings::WebVowl::get_alpha) | Annealing temperature |
//! | `setCenter(x, y)` | [`set_center`](bindings::WebVowl::set_center) | Move the layout centre |
//! | `setLinkDistance(d)` | [`set_link_distance`](bindings::WebVowl::set_link_distance) | Spring rest length |
//! | `setChargeStrength(s)` | [`set_charge_strength`](bindings::WebVowl::set_charge_strength) | Repulsion strength |
//! | `getNodeCount()` | [`get_node_count`](bindings::WebVowl::get_node_count) | Node count |
//! | `getEdgeCount()` | [`get_edge_count`](bindings::WebVowl::get_edge_count) | Edge count |
//! | `getNodeIds()` | [`get_node_ids`](bindings::WebVowl::get_node_ids) | Node ids, in index order |
//! | `getNodePositions()` | [`get_node_positions`](bindings::WebVowl::get_node_positions) | Live `x, y` per node as a `Float32Array` |
//! | `writePositionsInto(buf)` | [`write_positions_into`](bindings::WebVowl::write_positions_into) | Copy those positions into a caller-owned buffer |
//! | `getStatistics()` | [`get_statistics`](bindings::WebVowl::get_statistics) | Degree, components, OWL2 metrics |
//! | `getMetadata()` | [`get_metadata`](bindings::WebVowl::get_metadata) | Ontology header (IRI, version, title) |
//!
//! Filtering, pinning and folding (`filterByDegree`, `toggleDatatypes`,
//! `toggleSetOperators`, `resetVisibility`, `filterHierarchy`, `pinNode`,
//! `unpinNode`, `isPinned`, `resetPins`, `getPinnedCount`, `getNodeDegree`) are
//! also always available; see [`bindings::WebVowl`] for the full list.
//!
//! ## Feature-gated
//!
//! | JavaScript | Feature | Purpose |
//! |---|---|---|
//! | `getGraphData()` | `debug-serde` | Full-graph serde export, for diagnostics |
//! | `parseMarkdownOntology(md)` | `markdown-ontology` | Parse a markdown ontology block |
//! | `validateOWL2(md)` | `markdown-ontology` | OWL2 best-practice validation |
//! | `checkNodeClick(origin, dir)` | `interaction` | Ray-sphere node hit testing |
//! | `new NggExplorer()` and its methods | `ngg1` | Zero-copy worker physics |
//!
//! # Features
//!
//! All optional features are **off by default**; the general-purpose engine —
//! OWL/JSON parsing, the graph model, force layout (Barnes-Hut quadtree and the
//! CSR simulation), OWL2 validation, pinning and statistics — is always compiled.
//!
//! - **`ngg1`** — the NGG1 binary tier format and the
//!   [`NggExplorer`](bindings::explorer::NggExplorer) binding.
//! - **`markdown-ontology`** — the markdown ontology parser and the filesystem
//!   loader built on it. The only feature that needs `regex`, which is why it is
//!   not in the published npm bundle: it was 68% of the compiled code in 0.1.0.
//!   Build the bundle yourself with the feature on if you need it in a browser.
//! - **`interaction`** — ray-sphere hit testing for click and selection.
//! - **`debug-serde`** — the legacy full-graph serde export.
//! - **`parallel`** — data-parallel ontology loading via rayon. Native only.
//! - **`simd`** — WASM SIMD128 force accumulation. Needs the `simd128` target
//!   feature.
//!
//! # Licence
//!
//! MIT. See `LICENSE`, which carries both the upstream WebVOWL notice and
//! DreamLab-AI's.

#![deny(missing_docs)]
#![deny(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod bindings;
pub mod debug;
pub mod graph;
pub mod layout;
pub mod ontology;
pub mod render;

#[cfg(feature = "ngg1")]
#[cfg_attr(docsrs, doc(cfg(feature = "ngg1")))]
pub mod ngg1;

#[cfg(feature = "interaction")]
#[cfg_attr(docsrs, doc(cfg(feature = "interaction")))]
pub mod interaction;

mod error;

pub use error::{Result, VowlError};

use wasm_bindgen::prelude::*;

/// WASM module entry point, run once when the module is instantiated.
///
/// `wasm-bindgen` calls this automatically; there is no need to invoke it from
/// JavaScript. It is currently a no-op, reserved for panic-hook installation.
#[wasm_bindgen(start)]
pub fn init() {}

/// The crate version, as reported to JavaScript.
///
/// Returns the `CARGO_PKG_VERSION` this module was compiled from — useful for
/// asserting that the loaded `.wasm` matches the JavaScript glue shipped with it.
///
/// ```
/// assert_eq!(vowl_wasm::version(), env!("CARGO_PKG_VERSION"));
/// ```
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
