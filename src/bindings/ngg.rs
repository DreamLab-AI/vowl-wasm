//! Owned NGG1 graph snapshots and encoding for browser scope operations.
//!
//! These bindings delegate every binary read and write to [`crate::ngg1`].
//! Returned arrays and strings are independent snapshots; callers can free the
//! graph once they have copied the fields they need. Layout continues through
//! [`super::explorer::NggExplorer`] without serialising metadata per frame.

use crate::ngg1::{self, Csr, Graph, GraphNode, Ngg1, Ngg1Error};
use serde::Serialize;
use std::collections::BTreeMap;
use wasm_bindgen::prelude::*;

/// An owned, validated NGG1 graph. Created in JavaScript by `decodeNgg1`.
///
/// Numeric domain/category ids are supplied by the producer; this type does
/// not map them to an estate taxonomy. Every array accessor returns a copy.
#[wasm_bindgen]
pub struct NggGraph {
    graph: Graph,
}

impl NggGraph {
    /// Decode a graph through the central codec, including strict UTF-8 checks.
    ///
    /// # Errors
    /// Returns the codec's error for malformed sections, CSR or strings.
    ///
    /// ```
    /// use vowl_wasm::{bindings::ngg::NggGraph, ngg1};
    /// let graph = NggGraph::from_bytes(&ngg1::worked_example())?;
    /// assert_eq!(graph.labels(), vec!["AI", "ML", "NLP"]);
    /// assert_eq!(graph.row_ptr(), vec![0, 0, 2, 2]);
    /// # Ok::<(), ngg1::Ngg1Error>(())
    /// ```
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Ngg1Error> {
        Ok(Self {
            graph: Ngg1::parse(bytes)?.to_graph().map_err(Ngg1Error::from)?,
        })
    }
}

#[wasm_bindgen]
impl NggGraph {
    /// Number of nodes in local-index order.
    #[wasm_bindgen(js_name = nodeCount)]
    pub fn node_count(&self) -> usize {
        self.graph.nodes.len()
    }

    /// Number of directed CSR edges.
    #[wasm_bindgen(js_name = edgeCount)]
    pub fn edge_count(&self) -> usize {
        self.graph.csr.col_idx.len()
    }

    /// Stable external node ids, which need not equal local indices.
    pub fn ids(&self) -> Vec<u32> {
        self.graph.nodes.iter().map(|n| n.id).collect()
    }

    /// Seed positions packed as `[x0, y0, x1, y1, ...]`.
    pub fn positions(&self) -> Vec<f32> {
        self.graph.nodes.iter().flat_map(|n| [n.x, n.y]).collect()
    }

    /// Producer-defined domain ids; `DOMAIN_NONE` denotes no domain.
    pub fn domains(&self) -> Vec<u16> {
        self.graph.nodes.iter().map(|n| n.domain).collect()
    }

    /// Producer-defined category ids; `CATEGORY_NONE` denotes no category.
    pub fn categories(&self) -> Vec<u16> {
        self.graph.nodes.iter().map(|n| n.category).collect()
    }

    /// Node flag bitfields, including unknown bits preserved by the codec.
    pub fn flags(&self) -> Vec<u8> {
        self.graph.nodes.iter().map(|n| n.flags).collect()
    }

    /// Full-graph incident degrees in local-index order.
    pub fn degrees(&self) -> Vec<u32> {
        self.graph.nodes.iter().map(|n| n.degree).collect()
    }

    /// UTF-8 display labels in local-index order.
    pub fn labels(&self) -> Vec<String> {
        self.graph.nodes.iter().map(|n| n.label.clone()).collect()
    }

    /// Canonical IRIs in local-index order.
    pub fn iris(&self) -> Vec<String> {
        self.graph.nodes.iter().map(|n| n.iri.clone()).collect()
    }

    /// CSR row offsets, of length `nodeCount() + 1`.
    #[wasm_bindgen(js_name = rowPtr)]
    pub fn row_ptr(&self) -> Vec<u32> {
        self.graph.csr.row_ptr.clone()
    }

    /// CSR target local indices, aligned with `edgeType()`.
    #[wasm_bindgen(js_name = colIdx)]
    pub fn col_idx(&self) -> Vec<u32> {
        self.graph.csr.col_idx.clone()
    }

    /// Edge type codes, aligned with `colIdx()`.
    #[wasm_bindgen(js_name = edgeType)]
    pub fn edge_type(&self) -> Vec<u8> {
        self.graph.csr.edge_type.clone()
    }
}

/// Decode a tier into an owned JavaScript `NggGraph`.
///
/// # Errors
/// Throws an `NGG1: ...` string when sections, topology or UTF-8 are invalid.
#[wasm_bindgen(js_name = decodeNgg1)]
pub fn decode_ngg1(bytes: &[u8]) -> Result<NggGraph, JsValue> {
    NggGraph::from_bytes(bytes).map_err(|e| JsValue::from_str(&e.to_string()))
}

struct GraphParts<'a> {
    seed: &'a [f32],
    ids: &'a [u32],
    domains: &'a [u16],
    categories: &'a [u16],
    flags: &'a [u8],
    degrees: &'a [u32],
    labels: Vec<String>,
    iris: Vec<String>,
    csr: Csr,
}

fn encode_parts(p: GraphParts<'_>) -> Result<Vec<u8>, String> {
    let n = p.ids.len();
    let positions = n.checked_mul(2).ok_or("NGG1: node count overflow")?;
    for (name, found, expected) in [
        ("seed", p.seed.len(), positions),
        ("domains", p.domains.len(), n),
        ("categories", p.categories.len(), n),
        ("flags", p.flags.len(), n),
        ("degrees", p.degrees.len(), n),
        ("labels", p.labels.len(), n),
        ("iris", p.iris.len(), n),
    ] {
        if found != expected {
            return Err(format!("NGG1: {name} length {found}, expected {expected}"));
        }
    }
    let nodes = p
        .labels
        .into_iter()
        .zip(p.iris)
        .enumerate()
        .map(|(i, (label, iri))| GraphNode {
            id: p.ids[i],
            x: p.seed[i * 2],
            y: p.seed[i * 2 + 1],
            domain: p.domains[i],
            category: p.categories[i],
            flags: p.flags[i],
            degree: p.degrees[i],
            label,
            iri,
        })
        .collect::<Vec<_>>();
    ngg1::encode(&nodes, &p.csr).map_err(|e| format!("NGG1: {e}"))
}

/// Encode an explicit node set and CSR through ontomark's NGG1 writer.
///
/// `seed` is packed `[x0,y0,...]`. Every other node array and both string
/// arrays must have the same length as `ids`. CSR targets are local indices;
/// ids remain external identifiers. Unknown taxonomy ids, flags and edge
/// codes are preserved. The result is a standalone `Uint8Array`.
///
/// # Errors
/// Throws an `NGG1: ...` string for mismatched arrays, invalid CSR or counts
/// exceeding the codec's limits. No section offsets are calculated here.
#[wasm_bindgen(js_name = encodeNgg1)]
#[allow(clippy::too_many_arguments)]
pub fn encode_ngg1(
    seed: &[f32],
    ids: &[u32],
    domains: &[u16],
    categories: &[u16],
    flags: &[u8],
    degrees: &[u32],
    labels: Vec<String>,
    iris: Vec<String>,
    row_ptr: &[u32],
    col_idx: &[u32],
    edge_type: &[u8],
) -> Result<Vec<u8>, JsValue> {
    encode_parts(GraphParts {
        seed,
        ids,
        domains,
        categories,
        flags,
        degrees,
        labels,
        iris,
        csr: Csr {
            row_ptr: row_ptr.to_vec(),
            col_idx: col_idx.to_vec(),
            edge_type: edge_type.to_vec(),
        },
    })
    .map_err(|e| JsValue::from_str(&e))
}

fn constants() -> BTreeMap<&'static str, u32> {
    [
        ("NGG1_MAGIC", ngg1::NGG1_MAGIC),
        ("NGG1_VERSION", u32::from(ngg1::NGG1_VERSION)),
        ("NGG1_HEADER_SIZE", ngg1::NGG1_HEADER_SIZE as u32),
        ("NGG1_NODE_STRIDE", ngg1::NGG1_NODE_STRIDE as u32),
        ("EDGE_SUBCLASS", u32::from(ngg1::EDGE_SUBCLASS)),
        ("EDGE_RELATION", u32::from(ngg1::EDGE_RELATION)),
        ("FLAG_DOMAIN_ROOT", u32::from(ngg1::FLAG_DOMAIN_ROOT)),
        ("FLAG_CATEGORY_ROOT", u32::from(ngg1::FLAG_CATEGORY_ROOT)),
        ("FLAG_HAS_PAGE", u32::from(ngg1::FLAG_HAS_PAGE)),
        ("FLAG_BRIDGE", u32::from(ngg1::FLAG_BRIDGE)),
        ("FLAG_INDIVIDUAL", u32::from(ngg1::FLAG_INDIVIDUAL)),
        ("FLAG_META", u32::from(ngg1::FLAG_META)),
        ("DOMAIN_NONE", u32::from(ngg1::DOMAIN_NONE)),
        ("CATEGORY_NONE", u32::from(ngg1::CATEGORY_NONE)),
    ]
    .into_iter()
    .collect()
}

/// Return format constants directly from the central NGG1 owner.
///
/// Call this after WASM initialisation instead of duplicating flags, sentinels
/// or edge codes in JavaScript. The returned object is independent of WASM.
#[wasm_bindgen(js_name = ngg1Constants, unchecked_return_type = "Ngg1Constants")]
pub fn ngg1_constants() -> JsValue {
    constants()
        .serialize(&serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true))
        .expect("constant string keys and u32 values are serialisable")
}

#[wasm_bindgen(typescript_custom_section)]
const TYPES: &str = r#"
export interface Ngg1Constants {
    readonly NGG1_MAGIC: number;
    readonly NGG1_VERSION: number;
    readonly NGG1_HEADER_SIZE: number;
    readonly NGG1_NODE_STRIDE: number;
    readonly EDGE_SUBCLASS: number;
    readonly EDGE_RELATION: number;
    readonly FLAG_DOMAIN_ROOT: number;
    readonly FLAG_CATEGORY_ROOT: number;
    readonly FLAG_HAS_PAGE: number;
    readonly FLAG_BRIDGE: number;
    readonly FLAG_INDIVIDUAL: number;
    readonly FLAG_META: number;
    readonly DOMAIN_NONE: number;
    readonly CATEGORY_NONE: number;
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn example<'a>(seed: &'a [f32]) -> GraphParts<'a> {
        GraphParts {
            seed,
            ids: &[0, 1, 2],
            domains: &[0, 0, 0],
            categories: &[0, 1, 1],
            flags: &[1, 0, 0],
            degrees: &[1, 2, 1],
            labels: vec!["AI".into(), "ML".into(), "NLP".into()],
            iris: vec!["ng:0".into(), "ng:1".into(), "ng:2".into()],
            csr: Csr {
                row_ptr: vec![0, 0, 2, 2],
                col_idx: vec![0, 2],
                edge_type: vec![0, 1],
            },
        }
    }

    #[test]
    fn binding_encoder_writes_the_owner_golden_byte_for_byte() {
        assert_eq!(
            include_bytes!("../../tests/fixtures/ngg1-3n2e.bin").as_slice(),
            ngg1::worked_example()
        );
        let seed = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0];
        assert_eq!(
            encode_parts(example(&seed)).unwrap(),
            ngg1::worked_example()
        );
    }

    #[test]
    fn decoded_arrays_preserve_all_fields_and_are_independent() {
        let decoded = NggGraph::from_bytes(&ngg1::worked_example()).unwrap();
        assert_eq!(decoded.ids(), [0, 1, 2]);
        assert_eq!(decoded.positions(), [0.0, 0.0, 1.0, 0.0, 0.0, 1.0]);
        assert_eq!(decoded.domains(), [0, 0, 0]);
        assert_eq!(decoded.categories(), [0, 1, 1]);
        assert_eq!(decoded.flags(), [1, 0, 0]);
        assert_eq!(decoded.degrees(), [1, 2, 1]);
        assert_eq!(decoded.labels(), ["AI", "ML", "NLP"]);
        assert_eq!(decoded.iris(), ["ng:0", "ng:1", "ng:2"]);
        let mut row = decoded.row_ptr();
        row[0] = 99;
        assert_eq!(decoded.row_ptr(), [0, 0, 2, 2]);
        assert_eq!(decoded.col_idx(), [0, 2]);
        assert_eq!(decoded.edge_type(), [0, 1]);
    }

    #[test]
    fn encode_rejects_short_arrays_and_invalid_csr_without_panics() {
        let seed = [0.0; 6];
        let mut parts = example(&seed);
        parts.labels.pop();
        assert!(encode_parts(parts).unwrap_err().contains("labels length"));
        assert!(encode_parts(example(&seed[..5]))
            .unwrap_err()
            .contains("seed length"));
        let mut parts = example(&seed);
        parts.csr.col_idx[0] = 99;
        assert!(encode_parts(parts).unwrap_err().contains("targets node 99"));
        let mut parts = example(&seed);
        parts.csr.row_ptr[0] = 1;
        assert!(encode_parts(parts).unwrap_err().contains("CSR invariants"));
    }

    #[test]
    fn non_estate_unicode_graph_roundtrips_with_unknown_codes() {
        let seed = [3.25, -7.5, 8.0, 9.0, 10.0, 11.0];
        let mut parts = example(&seed);
        parts.domains = &[12, 99, ngg1::DOMAIN_NONE];
        parts.categories = &[45, ngg1::CATEGORY_NONE, 2345];
        parts.flags = &[0x80, ngg1::FLAG_META, 0];
        parts.labels = vec!["Café".into(), "星雲".into(), "".into()];
        parts.iris = vec![
            "urn:custom:α".into(),
            "https://example.org/星".into(),
            "".into(),
        ];
        parts.csr.edge_type[1] = 99;
        let bytes = encode_parts(parts).unwrap();
        let decoded = NggGraph::from_bytes(&bytes).unwrap();
        assert_eq!(decoded.domains(), [12, 99, ngg1::DOMAIN_NONE]);
        assert_eq!(decoded.categories(), [45, ngg1::CATEGORY_NONE, 2345]);
        assert_eq!(decoded.flags(), [0x80, ngg1::FLAG_META, 0]);
        assert_eq!(decoded.positions(), seed);
        assert_eq!(decoded.labels(), ["Café", "星雲", ""]);
        assert_eq!(decoded.edge_type(), [0, 99]);
        assert_eq!(decoded.graph.encode().unwrap(), bytes);
    }

    #[test]
    fn constants_are_owner_values_and_decode_rejects_malformed_bytes() {
        assert_eq!(constants()["CATEGORY_NONE"], u32::from(ngg1::CATEGORY_NONE));
        assert_eq!(constants()["FLAG_META"], u32::from(ngg1::FLAG_META));
        assert!(NggGraph::from_bytes(&[]).is_err());
        let mut bytes = ngg1::worked_example();
        *bytes.last_mut().unwrap() = 0xff;
        assert!(NggGraph::from_bytes(&bytes).is_err());
    }
}
