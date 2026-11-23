//! The 0.3.0 layout, frozen.
//!
//! `fixtures/layout-0.3.0-golden.json` holds, for every ontology fixture, the
//! settled positions (as `f32` bit patterns, which is what
//! `getNodePositions` returns) and the tick count of the 0.3.0 engine, under
//! its default settings and under the settings the WasmVOWL explorer runs
//! (`setLinkDistance(100)`, `setChargeStrength(-300)`). It was written by this
//! test against the 0.3.0 release commit (`1912cf3`) with
//! `VOWL_WRITE_LAYOUT_GOLDEN=1`.
//!
//! Collision, positional gravity and the annealing setters added later are off
//! by default, so a caller that never touches them must get the 0.3.0 layout
//! bit for bit.

use std::collections::BTreeMap;

use serde_json::{json, Value};
use vowl_wasm::bindings::WebVowl;

const FIXTURES: &[(&str, &str)] = &[
    (
        "benchmark",
        include_str!("fixtures/ontologies/benchmark.json"),
    ),
    ("foaf", include_str!("fixtures/ontologies/foaf.json")),
    (
        "goodrelations",
        include_str!("fixtures/ontologies/goodrelations.json"),
    ),
    ("muto", include_str!("fixtures/ontologies/muto.json")),
    (
        "ontovibe",
        include_str!("fixtures/ontologies/ontovibe.json"),
    ),
    (
        "personasonto",
        include_str!("fixtures/ontologies/personasonto.json"),
    ),
    ("sioc", include_str!("fixtures/ontologies/sioc.json")),
];

/// The `simd` feature sums the small-graph repulsion in a different order, so
/// its layouts differ from the scalar build's in the last bits (already so in
/// 0.3.0) and it has its own golden file, recorded from the same commit with
/// `--features simd`. The published bundle is built without `simd`.
#[cfg(not(feature = "simd"))]
const GOLDEN_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/layout-0.3.0-golden.json"
);
#[cfg(feature = "simd")]
const GOLDEN_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/layout-0.3.0-golden-simd.json"
);

/// A named way of configuring the engine before `loadOntology`.
type Profile = (&'static str, fn(&mut WebVowl));

const PROFILES: &[Profile] = &[
    ("default", |_| {}),
    ("explorer", |v| {
        v.set_link_distance(100.0);
        v.set_charge_strength(-300.0);
    }),
];

/// Settle `json` under `configure` and return `(ticks, f32 bit patterns)`.
fn settle(json: &str, configure: fn(&mut WebVowl)) -> (usize, Vec<u32>) {
    let mut vowl = WebVowl::new();
    configure(&mut vowl);
    vowl.load_ontology(json).expect("fixture loads");
    vowl.init_simulation().expect("simulation initialises");
    let mut ticks = 0;
    while !vowl.is_finished() {
        assert!(ticks < 10_000, "did not finish");
        vowl.tick().expect("tick");
        ticks += 1;
    }
    let bits = vowl
        .get_node_positions()
        .iter()
        .map(|v| v.to_bits())
        .collect();
    (ticks, bits)
}

fn current() -> Value {
    let mut out = BTreeMap::new();
    for &(profile, configure) in PROFILES {
        for &(name, json) in FIXTURES {
            let (ticks, bits) = settle(json, configure);
            let hex: Vec<String> = bits.iter().map(|b| format!("{b:08x}")).collect();
            out.insert(
                format!("{profile}/{name}"),
                json!({ "ticks": ticks, "positions": hex }),
            );
        }
    }
    json!(out)
}

#[test]
fn default_settings_reproduce_the_0_3_0_layout_exactly() {
    let now = current();
    if std::env::var_os("VOWL_WRITE_LAYOUT_GOLDEN").is_some() {
        std::fs::write(
            GOLDEN_PATH,
            serde_json::to_string_pretty(&now).unwrap() + "\n",
        )
        .unwrap();
        return;
    }
    let golden: Value =
        serde_json::from_str(&std::fs::read_to_string(GOLDEN_PATH).expect("golden file present"))
            .unwrap();
    let golden = golden.as_object().unwrap();
    let now = now.as_object().unwrap();
    assert_eq!(
        golden.keys().collect::<Vec<_>>(),
        now.keys().collect::<Vec<_>>()
    );
    let mut mismatches = Vec::new();
    for (key, want) in golden {
        let got = &now[key];
        if want["ticks"] != got["ticks"] {
            mismatches.push(format!(
                "{key}: {} ticks, 0.3.0 took {}",
                got["ticks"], want["ticks"]
            ));
        }
        let (w, g) = (
            want["positions"].as_array().unwrap(),
            got["positions"].as_array().unwrap(),
        );
        let differing = w.iter().zip(g).filter(|(a, b)| a != b).count();
        if w.len() != g.len() || differing > 0 {
            mismatches.push(format!(
                "{key}: {differing} of {} coordinates differ from 0.3.0",
                w.len()
            ));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}
