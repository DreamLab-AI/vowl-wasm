//! Readability forces: collision, positional gravity and the annealing
//! schedule, on the seven real ontology fixtures.
//!
//! The settings mirror the WasmVOWL explorer (`setLinkDistance(100)`,
//! `setChargeStrength(-300)`), whose class nodes are drawn with a radius of 20
//! units. Every scenario must still give a finite, bounded layout that has
//! stopped moving; on top of that:
//!
//! - with collision on, no two nodes overlap: each pair is at least the sum of
//!   their radii apart, less [`OVERLAP_TOLERANCE`];
//! - with gravity on, every disconnected component's centroid sits within
//!   [`COMPONENT_REACH`] of the main component, instead of wherever the seed
//!   circle and the charge left it;
//! - a slower alpha decay runs past the default 300 ticks and still settles.
//!
//! Run with `--nocapture` to see the measured distances.

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

const LINK_DISTANCE: f64 = 100.0;
const CHARGE: f64 = -300.0;
/// The radius WasmVOWL draws a class node with.
const RADIUS: f64 = 20.0;
/// Overlap allowed below the sum of two radii, as a fraction of that sum.
const OVERLAP_TOLERANCE: f64 = 0.05;
/// Gravity strength used by the component tests (d3 `forceX` scale).
const GRAVITY: f64 = 0.05;
/// A component's centroid may be at most this far outside the main
/// component's own radius (its farthest node from its centroid), in units of
/// the link distance.
const COMPONENT_REACH: f64 = 1.5;
/// Hard cap on ticks for any run in this file.
const MAX_TICKS: usize = 5_000;

/// `SPREAD_FACTOR` of the convergence suite: bound on |coordinate| in units of
/// `link_distance * sqrt(n)`.
const SPREAD_FACTOR: f64 = 10.0;

type Point = (f64, f64);

fn dist(a: Point, b: Point) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

fn positions(vowl: &WebVowl) -> Vec<Point> {
    vowl.get_node_positions()
        .chunks_exact(2)
        .map(|xy| (f64::from(xy[0]), f64::from(xy[1])))
        .collect()
}

struct Settled {
    vowl: WebVowl,
    positions: Vec<Point>,
    ticks: usize,
    /// Mean per-node displacement of the final tick.
    last_step: f64,
    /// Largest |coordinate| on any tick.
    peak: f64,
}

fn settle(json: &str, configure: impl FnOnce(&mut WebVowl)) -> Settled {
    let mut vowl = WebVowl::new();
    vowl.set_link_distance(LINK_DISTANCE);
    vowl.set_charge_strength(CHARGE);
    vowl.load_ontology(json).expect("fixture loads");
    configure(&mut vowl);
    vowl.init_simulation().expect("initialise");
    let mut previous = positions(&vowl);
    let (mut ticks, mut last_step, mut peak) = (0usize, 0.0, 0.0f64);
    while !vowl.is_finished() {
        assert!(ticks < MAX_TICKS, "still running after {MAX_TICKS} ticks");
        vowl.tick().expect("tick");
        ticks += 1;
        let now = positions(&vowl);
        last_step = previous
            .iter()
            .zip(&now)
            .map(|(a, b)| dist(*a, *b))
            .sum::<f64>()
            / now.len() as f64;
        for &(x, y) in &now {
            peak = peak.max(x.abs()).max(y.abs());
        }
        previous = now;
    }
    Settled {
        vowl,
        positions: previous,
        ticks,
        last_step,
        peak,
    }
}

fn assert_bounded_and_settled(name: &str, run: &Settled) {
    let n = run.positions.len();
    let bound = SPREAD_FACTOR * LINK_DISTANCE * (n as f64).sqrt();
    for &(x, y) in &run.positions {
        assert!(x.is_finite() && y.is_finite(), "{name}: node at ({x}, {y})");
    }
    assert!(
        run.peak <= bound,
        "{name}: peak |coordinate| {:.1} exceeds {bound:.0}",
        run.peak
    );
    assert!(
        run.last_step < 0.01 * LINK_DISTANCE,
        "{name}: still moving {:.3} per node on the final tick",
        run.last_step
    );
}

/// Smallest `d(i, j) - (r_i + r_j)` over all pairs, with the pair count under
/// `threshold` for reporting.
fn min_pair(positions: &[Point], threshold: f64) -> (f64, usize) {
    let mut closest = f64::INFINITY;
    let mut under = 0;
    for i in 0..positions.len() {
        for j in (i + 1)..positions.len() {
            let d = dist(positions[i], positions[j]);
            closest = closest.min(d);
            if d < threshold {
                under += 1;
            }
        }
    }
    (closest, under)
}

/// Connected components of the fixture's edge set, as node index lists,
/// largest first.
fn components(json: &str, ids: &[String]) -> Vec<Vec<usize>> {
    let doc: serde_json::Value = serde_json::from_str(json).unwrap();
    let index: std::collections::HashMap<&str, usize> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i))
        .collect();
    let mut parent: Vec<usize> = (0..ids.len()).collect();
    fn find(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    for prop in doc["property"].as_array().unwrap() {
        let (Some(&a), Some(&b)) = (
            index.get(prop["domain"].as_str().unwrap()),
            index.get(prop["range"].as_str().unwrap()),
        ) else {
            continue;
        };
        let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
        parent[ra] = rb;
    }
    let mut groups: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    for i in 0..ids.len() {
        let root = find(&mut parent, i);
        groups.entry(root).or_default().push(i);
    }
    let mut out: Vec<Vec<usize>> = groups.into_values().collect();
    out.sort_by_key(|c| std::cmp::Reverse(c.len()));
    out
}

fn centroid(positions: &[Point], members: &[usize]) -> Point {
    let n = members.len() as f64;
    let (sx, sy) = members.iter().fold((0.0, 0.0), |(x, y), &i| {
        (x + positions[i].0, y + positions[i].1)
    });
    (sx / n, sy / n)
}

/// For each disconnected component, how far its centroid sits outside the
/// main component's radius; and that radius.
fn component_overshoot(json: &str, run: &Settled) -> (f64, Vec<f64>) {
    let comps = components(json, &run.vowl.get_node_ids());
    let main = &comps[0];
    let c = centroid(&run.positions, main);
    let radius = main
        .iter()
        .map(|&i| dist(run.positions[i], c))
        .fold(0.0, f64::max);
    let overshoot = comps[1..]
        .iter()
        .map(|comp| dist(centroid(&run.positions, comp), c) - radius)
        .collect();
    (radius, overshoot)
}

#[test]
fn collision_keeps_every_pair_two_radii_apart() {
    for &(name, json) in FIXTURES {
        let before = settle(json, |_| {});
        let after = settle(json, |v| v.set_collision_radius(RADIUS));
        let (before_min, before_under) = min_pair(&before.positions, 2.0 * RADIUS);
        let (after_min, after_under) = min_pair(&after.positions, 2.0 * RADIUS);
        eprintln!(
            "{name}: min pair distance {before_min:.1} -> {after_min:.1}; \
             pairs under {:.0}: {before_under} -> {after_under}; ticks {}",
            2.0 * RADIUS,
            after.ticks
        );
        assert_bounded_and_settled(name, &after);
        assert!(
            after_min >= 2.0 * RADIUS * (1.0 - OVERLAP_TOLERANCE),
            "{name}: two nodes {after_min:.2} apart with collision radius {RADIUS}"
        );
    }
}

#[test]
fn per_node_radii_are_honoured() {
    for &(name, json) in FIXTURES {
        let mut radii = Vec::new();
        let run = settle(json, |v| {
            // Three sizes, by index, so neighbours differ.
            radii = (0..v.get_node_count())
                .map(|i| [12.0, 20.0, 32.0][i % 3])
                .collect();
            v.set_node_radii(&radii).expect("one radius per node");
            // Mixed sizes jam the dense graphs (goodrelations settles at 0.89
            // of the radius sum with one pass); four passes per tick resolve
            // them, as d3-force's `iterations` is meant to.
            v.set_collision_iterations(4);
        });
        assert_bounded_and_settled(name, &run);
        let mut worst = f64::INFINITY;
        for i in 0..run.positions.len() {
            for j in (i + 1)..run.positions.len() {
                let ratio = dist(run.positions[i], run.positions[j]) / (radii[i] + radii[j]);
                worst = worst.min(ratio);
            }
        }
        eprintln!("{name}: worst distance / (r_i + r_j) = {worst:.3}");
        assert!(
            worst >= 1.0 - OVERLAP_TOLERANCE,
            "{name}: a pair sits at {worst:.3} of its radius sum"
        );
    }
}

#[test]
fn gravity_packs_disconnected_components_around_the_main_one() {
    for &(name, json) in FIXTURES {
        let before = settle(json, |_| {});
        let after = settle(json, |v| {
            v.set_collision_radius(RADIUS);
            v.set_gravity_strength(GRAVITY);
        });
        let (r0, o0) = component_overshoot(json, &before);
        let (r1, o1) = component_overshoot(json, &after);
        let worst = |o: &[f64]| o.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        eprintln!(
            "{name}: {} extra components; main radius {r0:.0} -> {r1:.0}; \
             worst centroid beyond main radius {:.0} -> {:.0}",
            o1.len(),
            worst(&o0),
            worst(&o1)
        );
        assert_bounded_and_settled(name, &after);
        for (k, &over) in o1.iter().enumerate() {
            assert!(
                over <= COMPONENT_REACH * LINK_DISTANCE,
                "{name}: component {} centroid {over:.0} beyond the main component's radius {r1:.0}",
                k + 1
            );
        }
    }
}

#[test]
fn a_slower_alpha_decay_runs_past_300_ticks_and_settles() {
    for &(name, json) in FIXTURES {
        let run = settle(json, |v| {
            v.set_alpha_decay(0.01);
            v.set_collision_radius(RADIUS);
        });
        assert!(
            run.ticks > 300,
            "{name}: finished after {} ticks",
            run.ticks
        );
        assert_bounded_and_settled(name, &run);
    }
}

#[test]
fn alpha_min_and_velocity_decay_are_applied() {
    let (_, json) = FIXTURES[1];
    let default = settle(json, |_| {});
    let longer = settle(json, |v| v.set_alpha_min(0.0001));
    assert!(
        longer.ticks > default.ticks,
        "alpha_min 1e-4: {} ticks vs {}",
        longer.ticks,
        default.ticks
    );
    // More friction (less velocity kept) must still settle.
    let damped = settle(json, |v| v.set_velocity_decay(0.4));
    assert_bounded_and_settled("foaf damped", &damped);
}

#[test]
fn bad_schedule_values_are_ignored() {
    let (_, json) = FIXTURES[3];
    let reference = settle(json, |_| {});
    let run = settle(json, |v| {
        v.set_alpha_decay(f64::NAN);
        v.set_alpha_decay(1.5);
        v.set_alpha_decay(-0.1);
        v.set_alpha_min(0.0);
        v.set_alpha_min(f64::INFINITY);
        v.set_velocity_decay(-1.0);
        v.set_velocity_decay(2.0);
        v.set_collision_radius(f64::NAN);
        v.set_collision_radius(-5.0);
        v.set_collision_strength(f64::NAN);
        v.set_gravity_strength(f64::NAN);
        v.set_collision_iterations(0);
    });
    assert_eq!(run.ticks, reference.ticks);
    assert_eq!(run.positions, reference.positions);
}

#[test]
fn collision_and_gravity_settings_survive_load() {
    // Like link distance and charge, settings made before `loadOntology` stay
    // in force for the next graph; per-node radii belong to one graph and are
    // cleared by a load.
    let (_, json) = FIXTURES[1];
    let configured_before = {
        let mut vowl = WebVowl::new();
        vowl.set_link_distance(LINK_DISTANCE);
        vowl.set_charge_strength(CHARGE);
        vowl.set_collision_radius(RADIUS);
        vowl.set_gravity_strength(GRAVITY);
        vowl.load_ontology(json).unwrap();
        vowl.run_simulation(MAX_TICKS).unwrap();
        positions(&vowl)
    };
    let configured_after = settle(json, |v| {
        v.set_collision_radius(RADIUS);
        v.set_gravity_strength(GRAVITY);
    });
    assert_eq!(configured_before, configured_after.positions);

    let mut vowl = WebVowl::new();
    vowl.load_ontology(json).unwrap();
    let n = vowl.get_node_count();
    vowl.set_node_radii(&vec![RADIUS; n]).unwrap();
    vowl.load_ontology(FIXTURES[3].1).unwrap();
    // The old radii would no longer match the node count; a load clears them,
    // so the next graph runs without collision rather than erroring.
    vowl.run_simulation(MAX_TICKS).unwrap();
    assert!(vowl.is_finished());
}
