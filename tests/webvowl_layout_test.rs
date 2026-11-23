//! Layout convergence of the `WebVowl` force simulation on real ontologies.
//!
//! Each fixture under `tests/fixtures/ontologies/` is an owl2vowl document
//! merged into the engine contract (see the README there). The tests drive the
//! simulation through the same calls a JavaScript consumer makes
//! (`loadOntology` → `initSimulation` → `tick` until `isFinished`, or
//! `runSimulation`) and check that the settled layout is a layout: finite,
//! bounded, free of coincident nodes, converged, and with linked nodes closer
//! together than unlinked ones.

use std::collections::HashMap;

use vowl_wasm::bindings::WebVowl;

/// The real ontologies, as `(name, json)`.
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

/// `LayoutConfig::default().link_distance`, the rest length the default
/// simulation runs at.
const DEFAULT_LINK_DISTANCE: f64 = 30.0;

/// Bound on any coordinate's distance from the layout centre, in units of
/// `link_distance * sqrt(n)`.
///
/// A force layout of `n` nodes whose links sit near their rest length fills an
/// area of order `n * link_distance²`, so its radius is of order
/// `link_distance * sqrt(n)`. Repulsion inflates that by a small constant
/// factor; `10` leaves several times the observed spread as headroom while
/// still failing by many orders of magnitude on a diverging layout (the 0.2.3
/// engine reached 1e19 to 1e30 on these fixtures).
const SPREAD_FACTOR: f64 = 10.0;

/// Two nodes closer than this (in layout units) count as coincident.
const MIN_SEPARATION: f64 = 1e-3;

/// Hard cap on ticks: alpha decays from 1 to below 0.001 in about 300 ticks at
/// the default decay, so a simulation that is still running at this point has
/// failed to finish.
const MAX_TICKS: usize = 2_000;

/// Edge endpoints as `(domain, range)` node ids, read from the fixture.
fn edges_of(json: &str) -> Vec<(String, String)> {
    let doc: serde_json::Value = serde_json::from_str(json).expect("fixture is valid JSON");
    doc["property"]
        .as_array()
        .expect("fixture has a property array")
        .iter()
        .map(|p| {
            (
                p["domain"].as_str().expect("domain").to_owned(),
                p["range"].as_str().expect("range").to_owned(),
            )
        })
        .collect()
}

fn positions(vowl: &WebVowl) -> Vec<(f64, f64)> {
    vowl.get_node_positions()
        .chunks_exact(2)
        .map(|xy| (f64::from(xy[0]), f64::from(xy[1])))
        .collect()
}

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// What a settled run produced.
struct Run {
    positions: Vec<(f64, f64)>,
    /// Mean per-node displacement of each tick.
    displacement: Vec<f64>,
    /// Largest |coordinate| seen on any tick.
    peak: f64,
}

impl Run {
    /// First tick after which the mean per-node step stays below
    /// `0.01 * link_distance` for the rest of the run.
    fn settled_at(&self, link_distance: f64) -> Option<usize> {
        let still = 0.01 * link_distance;
        let moving = self.displacement.iter().rposition(|&d| d >= still);
        match moving {
            None => Some(1),
            Some(i) if i + 1 < self.displacement.len() => Some(i + 2),
            Some(_) => None,
        }
    }
}

/// Load `json` and tick it to completion, recording per-tick displacement.
fn tick_to_completion(json: &str, configure: impl FnOnce(&mut WebVowl)) -> (WebVowl, Run) {
    let mut vowl = WebVowl::new();
    configure(&mut vowl);
    vowl.load_ontology(json).expect("fixture loads");
    vowl.init_simulation().expect("simulation initialises");

    let mut previous = positions(&vowl);
    let mut displacement = Vec::new();
    let mut peak: f64 = 0.0;
    while !vowl.is_finished() {
        assert!(
            displacement.len() < MAX_TICKS,
            "simulation still running after {MAX_TICKS} ticks (alpha {})",
            vowl.get_alpha()
        );
        vowl.tick().expect("tick");
        let now = positions(&vowl);
        let moved: f64 = previous.iter().zip(&now).map(|(a, b)| dist(*a, *b)).sum();
        displacement.push(moved / now.len() as f64);
        for &(x, y) in &now {
            peak = peak.max(x.abs()).max(y.abs());
        }
        previous = now;
    }
    (
        vowl,
        Run {
            positions: previous,
            displacement,
            peak,
        },
    )
}

/// Every layout property the task asks of a settled simulation.
fn assert_sane_layout(name: &str, json: &str, run: &Run, ids: &[String], link_distance: f64) {
    let n = run.positions.len();
    assert_eq!(n, ids.len(), "{name}: one position per node id");

    // 1. Finite.
    for (i, &(x, y)) in run.positions.iter().enumerate() {
        assert!(
            x.is_finite() && y.is_finite(),
            "{name}: node {} at ({x}, {y})",
            ids[i]
        );
    }

    // 2. Bounded, on every tick, not only at the end.
    let bound = SPREAD_FACTOR * link_distance * (n as f64).sqrt();
    assert!(
        run.peak <= bound,
        "{name}: peak coordinate {:.3e} exceeds {bound:.0} \
         ({SPREAD_FACTOR} x link_distance {link_distance} x sqrt({n}))",
        run.peak
    );

    // 3. No coincident nodes.
    let mut closest = f64::INFINITY;
    for i in 0..n {
        for j in (i + 1)..n {
            closest = closest.min(dist(run.positions[i], run.positions[j]));
        }
    }
    assert!(
        closest > MIN_SEPARATION,
        "{name}: two nodes {closest:.2e} apart"
    );

    // 4. Converged: motion dies away as alpha decays.
    let d = &run.displacement;
    let quarter = (d.len() / 4).max(1);
    let early: f64 = d[..quarter].iter().sum::<f64>() / quarter as f64;
    let late: f64 = d[d.len() - quarter..].iter().sum::<f64>() / quarter as f64;
    let last = *d.last().expect("at least one tick");
    assert!(
        late < early,
        "{name}: mean displacement grew from {early:.3e} (first quarter) to {late:.3e} (last quarter)"
    );
    assert!(
        last < 0.01 * link_distance,
        "{name}: still moving {last:.3e} per node on the final tick"
    );

    // 5. Linked nodes sit closer than the average pair.
    let index: HashMap<&str, usize> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i))
        .collect();
    let edges: Vec<(usize, usize)> = edges_of(json)
        .iter()
        .filter_map(|(a, b)| Some((*index.get(a.as_str())?, *index.get(b.as_str())?)))
        .filter(|(a, b)| a != b)
        .collect();
    assert!(!edges.is_empty(), "{name}: fixture has edges");
    let edge_mean = edges
        .iter()
        .map(|&(a, b)| dist(run.positions[a], run.positions[b]))
        .sum::<f64>()
        / edges.len() as f64;
    let mut pair_sum = 0.0;
    let mut pairs = 0usize;
    for i in 0..n {
        for j in (i + 1)..n {
            pair_sum += dist(run.positions[i], run.positions[j]);
            pairs += 1;
        }
    }
    let pair_mean = pair_sum / pairs as f64;
    assert!(
        edge_mean < pair_mean,
        "{name}: mean edge length {edge_mean:.1} is not below mean pair distance {pair_mean:.1}"
    );
}

#[test]
fn every_fixture_settles_into_a_bounded_layout_with_default_settings() {
    let mut failures = Vec::new();
    for &(name, json) in FIXTURES {
        let result = std::panic::catch_unwind(|| {
            let (vowl, run) = tick_to_completion(json, |_| {});
            eprintln!(
                "{name}: {} nodes, isFinished after {} ticks, motion settled at tick {}, peak |coord| {:.3e}",
                run.positions.len(),
                run.displacement.len(),
                run.settled_at(DEFAULT_LINK_DISTANCE)
                    .map_or_else(|| "never".to_owned(), |t| t.to_string()),
                run.peak
            );
            assert_sane_layout(
                name,
                json,
                &run,
                &vowl.get_node_ids(),
                DEFAULT_LINK_DISTANCE,
            );
        });
        if let Err(panic) = result {
            let message = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_default();
            failures.push(message);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn run_simulation_settles_every_fixture() {
    for &(name, json) in FIXTURES {
        let mut vowl = WebVowl::new();
        vowl.load_ontology(json).expect("fixture loads");
        vowl.run_simulation(MAX_TICKS).expect("run");
        assert!(vowl.is_finished(), "{name}: not finished after {MAX_TICKS}");
        let bound = SPREAD_FACTOR * DEFAULT_LINK_DISTANCE * (vowl.get_node_count() as f64).sqrt();
        for (x, y) in positions(&vowl) {
            assert!(
                x.is_finite() && y.is_finite() && x.abs() <= bound && y.abs() <= bound,
                "{name}: node at ({x}, {y}), bound {bound:.0}"
            );
        }
    }
}

#[test]
fn settings_made_before_load_survive_it() {
    // The consumer configures the engine, then loads. A longer rest length set
    // before `loadOntology` must still be in force afterwards.
    let (_, json) = FIXTURES[1];
    let mean_edge = |link_distance: Option<f64>| {
        let (vowl, run) = tick_to_completion(json, |v| {
            if let Some(d) = link_distance {
                v.set_link_distance(d);
            }
        });
        let ids = vowl.get_node_ids();
        let index: HashMap<&str, usize> = ids
            .iter()
            .enumerate()
            .map(|(i, id)| (id.as_str(), i))
            .collect();
        let lengths: Vec<f64> = edges_of(json)
            .iter()
            .filter(|(a, b)| a != b)
            .map(|(a, b)| {
                dist(
                    run.positions[index[a.as_str()]],
                    run.positions[index[b.as_str()]],
                )
            })
            .collect();
        lengths.iter().sum::<f64>() / lengths.len() as f64
    };
    let default = mean_edge(None);
    let long = mean_edge(Some(4.0 * DEFAULT_LINK_DISTANCE));
    assert!(
        long > 2.0 * default,
        "link distance set before load was lost: mean edge {long:.1} vs default {default:.1}"
    );
}

#[test]
fn a_long_chain_spreads_out_instead_of_collapsing() {
    // Twelve classes in a subclass chain: bounded on 0.2.3 too, but there the
    // short-range charge pulled nodes together instead of apart.
    let classes: Vec<String> = (0..12)
        .map(|i| format!(r#"{{"id":"c{i}","type":"owl:Class","label":"C{i}"}}"#))
        .collect();
    let properties: Vec<String> = (0..11)
        .map(|i| {
            format!(
                r#"{{"id":"p{i}","type":"subclass","label":"sub","domain":"c{i}","range":"c{}"}}"#,
                i + 1
            )
        })
        .collect();
    let json = format!(
        r#"{{"class":[{}],"property":[{}]}}"#,
        classes.join(","),
        properties.join(",")
    );
    let (vowl, run) = tick_to_completion(&json, |_| {});
    assert_sane_layout(
        "chain",
        &json,
        &run,
        &vowl.get_node_ids(),
        DEFAULT_LINK_DISTANCE,
    );
}
