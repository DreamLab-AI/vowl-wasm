//! `CsrSimulation` (the engine behind `NggExplorer`) on the seven real
//! ontologies under `tests/fixtures/ontologies/`.
//!
//! Each fixture becomes the graph a published consumer builds: one node per
//! class, seeded on a radius-300 circle, and every object property between two
//! distinct classes stored in both directions (the CSR adjacency
//! `encodeNgg1` → `loadCsr` produces, which `load_arrays` reproduces edge for
//! edge).
//!
//! The golden buffers under `tests/fixtures/ngg-golden/` were captured from
//! vowl-wasm 0.3.0 before the opt-in readability forces existed. With those
//! forces at their defaults, the layout must stay byte-identical, so that no
//! consumer's layout moves on upgrade.

use std::collections::HashMap;
use std::f64::consts::PI;
use std::path::PathBuf;

use vowl_wasm::layout::csr_sim::CsrSimulation;

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

/// Tick counts at which golden buffers are pinned: mid-flight (every force
/// still large) and after the alpha floor (the layout a consumer shows).
const GOLDEN_TICKS: [usize; 2] = [50, 600];

/// A fixture as `load_arrays` input.
struct Scope {
    seed: Vec<f32>,
    edge_a: Vec<u32>,
    edge_b: Vec<u32>,
}

impl Scope {
    fn n(&self) -> usize {
        self.seed.len() / 2
    }
}

/// Build the consumer graph for one fixture (see the module docs).
fn scope(json: &str) -> Scope {
    let doc: serde_json::Value = serde_json::from_str(json).expect("fixture is JSON");
    let classes = doc["class"].as_array().expect("fixture has classes");
    let index: HashMap<&str, usize> = classes
        .iter()
        .enumerate()
        .map(|(i, c)| (c["id"].as_str().expect("class id is a string"), i))
        .collect();
    let n = classes.len();
    let mut adj: Vec<Vec<u32>> = vec![Vec::new(); n];
    for p in doc["property"].as_array().into_iter().flatten() {
        let (Some(d), Some(r)) = (p["domain"].as_str(), p["range"].as_str()) else {
            continue;
        };
        let (Some(&a), Some(&b)) = (index.get(d), index.get(r)) else {
            continue;
        };
        if a == b {
            continue;
        }
        adj[a].push(b as u32);
        adj[b].push(a as u32);
    }
    let mut seed = vec![0.0f32; n * 2];
    for i in 0..n {
        let t = 2.0 * PI * i as f64 / n as f64;
        seed[i * 2] = (300.0 * t.cos()) as f32;
        seed[i * 2 + 1] = (300.0 * t.sin()) as f32;
    }
    let mut edge_a = Vec::new();
    let mut edge_b = Vec::new();
    for (i, row) in adj.iter().enumerate() {
        for &j in row {
            edge_a.push(i as u32);
            edge_b.push(j);
        }
    }
    Scope {
        seed,
        edge_a,
        edge_b,
    }
}

fn load(s: &Scope, configure: impl FnOnce(&mut CsrSimulation)) -> CsrSimulation {
    let mut sim = CsrSimulation::new();
    configure(&mut sim);
    sim.load_arrays(
        s.seed.clone(),
        s.edge_a.clone(),
        s.edge_b.clone(),
        vec![0; s.edge_a.len()],
    );
    sim
}

fn positions(sim: &CsrSimulation) -> Vec<f32> {
    let mut out = vec![0.0f32; sim.positions_len()];
    sim.write_positions_into(&mut out);
    out
}

fn to_le_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn golden_path(name: &str, ticks: usize) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ngg-golden")
        .join(format!("{name}.t{ticks}.f32le"))
}

/// Positions after each of [`GOLDEN_TICKS`] with every parameter at its
/// default.
fn default_trajectory(s: &Scope) -> Vec<(usize, Vec<u8>)> {
    let mut sim = load(s, |_| {});
    let mut out = Vec::new();
    let mut t = 0;
    for &target in &GOLDEN_TICKS {
        while t < target {
            sim.tick();
            t += 1;
        }
        out.push((target, to_le_bytes(&positions(&sim))));
    }
    out
}

/// Regenerate the golden buffers. They pin the 0.3.0 engine, so this is only
/// for re-deriving them from a 0.3.0 checkout; on a later engine it would turn
/// the byte-identity test into a tautology. It refuses to write unless
/// `VOWL_BLESS_NGG_GOLDEN=1`:
/// `VOWL_BLESS_NGG_GOLDEN=1 cargo test --test ngg_explorer_layout_test -- --ignored bless_golden`.
#[test]
#[ignore = "writes the golden buffers; run once on the 0.3.0 engine"]
fn bless_golden() {
    if std::env::var("VOWL_BLESS_NGG_GOLDEN").as_deref() != Ok("1") {
        eprintln!("bless_golden: set VOWL_BLESS_NGG_GOLDEN=1 to overwrite the 0.3.0 goldens");
        return;
    }
    let dir = golden_path("x", 0)
        .parent()
        .map(PathBuf::from)
        .expect("dir");
    std::fs::create_dir_all(&dir).expect("create golden dir");
    for (name, json) in FIXTURES {
        for (ticks, bytes) in default_trajectory(&scope(json)) {
            std::fs::write(golden_path(name, ticks), bytes).expect("write golden");
        }
    }
}

#[test]
fn defaults_are_byte_identical_to_0_3_0() {
    for (name, json) in FIXTURES {
        for (ticks, got) in default_trajectory(&scope(json)) {
            let path = golden_path(name, ticks);
            let want = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert!(
                got == want,
                "{name}: positions after {ticks} ticks differ from the 0.3.0 golden"
            );
        }
    }
}

/// Connected components as lists of node indices, in first-index order.
fn components(s: &Scope) -> Vec<Vec<usize>> {
    let n = s.n();
    let mut adj = vec![Vec::new(); n];
    for (&a, &b) in s.edge_a.iter().zip(&s.edge_b) {
        adj[a as usize].push(b as usize);
    }
    let mut seen = vec![false; n];
    let mut out = Vec::new();
    for start in 0..n {
        if seen[start] {
            continue;
        }
        seen[start] = true;
        let mut comp = vec![start];
        let mut k = 0;
        while k < comp.len() {
            for &j in &adj[comp[k]] {
                if !seen[j] {
                    seen[j] = true;
                    comp.push(j);
                }
            }
            k += 1;
        }
        out.push(comp);
    }
    out
}

/// Readability metrics of a layout after a run.
#[derive(Debug)]
struct Metrics {
    peak: f32,
    min_d: f32,
    close40: usize,
    settle: Option<usize>,
    /// Largest distance from a component centroid to the overall centroid.
    comp_off: f32,
}

fn centroid(p: &[f32], nodes: impl Iterator<Item = usize>) -> (f32, f32) {
    let (mut x, mut y, mut k) = (0.0f64, 0.0f64, 0usize);
    for i in nodes {
        x += f64::from(p[i * 2]);
        y += f64::from(p[i * 2 + 1]);
        k += 1;
    }
    ((x / k as f64) as f32, (y / k as f64) as f32)
}

fn run(s: &Scope, ticks: usize, configure: impl FnOnce(&mut CsrSimulation)) -> (Vec<f32>, Metrics) {
    let mut sim = load(s, configure);
    let mut prev = positions(&sim);
    let mut settle = None;
    for t in 1..=ticks {
        sim.tick();
        let p = positions(&sim);
        let step = (0..s.n())
            .map(|i| (p[i * 2] - prev[i * 2]).hypot(p[i * 2 + 1] - prev[i * 2 + 1]))
            .fold(0.0f32, f32::max);
        if t > 1 && settle.is_none() && step < 1.0 {
            settle = Some(t);
        }
        prev = p;
    }
    let p = prev;
    let n = s.n();
    let (mut peak, mut min_d, mut close40) = (0.0f32, f32::INFINITY, 0);
    for i in 0..n {
        peak = peak.max(p[i * 2].abs()).max(p[i * 2 + 1].abs());
        for j in i + 1..n {
            let d = (p[i * 2] - p[j * 2]).hypot(p[i * 2 + 1] - p[j * 2 + 1]);
            min_d = min_d.min(d);
            if d < 40.0 {
                close40 += 1;
            }
        }
    }
    let (gx, gy) = centroid(&p, 0..n);
    let comp_off = components(s)
        .iter()
        .map(|c| {
            let (x, y) = centroid(&p, c.iter().copied());
            (x - gx).hypot(y - gy)
        })
        .fold(0.0f32, f32::max);
    (
        p,
        Metrics {
            peak,
            min_d,
            close40,
            settle,
            comp_off,
        },
    )
}

/// Print the readability table for the defaults and for each `;`-separated
/// `key=value,...` set in `NGG_SETTINGS`:
/// `NGG_SETTINGS='collideRadius=14' cargo test --test ngg_explorer_layout_test -- --ignored --nocapture report`
#[test]
#[ignore = "prints the readability table"]
fn report() {
    let extra = std::env::var("NGG_SETTINGS").unwrap_or_default();
    let sets = std::iter::once("").chain(extra.split(';').filter(|s| !s.is_empty()));
    for set in sets {
        let kv: Vec<(String, f32)> = set
            .split(',')
            .filter(|p| !p.is_empty())
            .map(|p| {
                let (k, v) = p.split_once('=').expect("key=value");
                (k.to_string(), v.parse().expect("number"))
            })
            .collect();
        println!("== {}", if set.is_empty() { "defaults" } else { set });
        for (name, json) in FIXTURES {
            let s = scope(json);
            let (_, m) = run(&s, 600, |sim| {
                for (k, v) in &kv {
                    sim.set_param(k, *v);
                }
            });
            println!(
                "{name:14} n={:3} comps={:2} peak={:6.1} minD={:5.1} <40={:4} settle={:?} compOff={:6.1}",
                s.n(),
                components(&s).len(),
                m.peak,
                m.min_d,
                m.close40,
                m.settle,
                m.comp_off
            );
        }
    }
}

/// The compact collision radius: the largest node ontomark-graph draws
/// (12 px) plus a 2-unit margin.
const COLLIDE_RADIUS: f32 = 14.0;

/// The recommended collision radius: no two nodes closer than 42 units.
const RECOMMENDED_RADIUS: f32 = 21.0;

/// `SimConfig::default().link_distance`.
const LINK_DISTANCE: f32 = 60.0;

/// The settings recommended to consumers (see the CHANGELOG).
const RECOMMENDED: &[(&str, f32)] = &[
    ("collideRadius", RECOMMENDED_RADIUS),
    ("collideStrength", 0.7),
    ("collideIterations", 3.0),
    ("componentGravity", 0.1),
];

/// Collision resolves overlaps to within this fraction of `2 · radius`.
const COLLIDE_TOLERANCE: f32 = 0.95;

#[test]
fn collision_keeps_nodes_apart_and_still_settles() {
    let collide_only: &[(&str, f32)] = &[("collideRadius", COLLIDE_RADIUS)];
    for (radius, settings) in [
        (COLLIDE_RADIUS, collide_only),
        (RECOMMENDED_RADIUS, RECOMMENDED),
    ] {
        let configure = |sim: &mut CsrSimulation| {
            for &(k, v) in settings {
                sim.set_param(k, v);
            }
        };
        for (name, json) in FIXTURES {
            let s = scope(json);
            let (p, m) = run(&s, 600, configure);
            assert!(
                p.iter().all(|v| v.is_finite()),
                "{name}: non-finite position"
            );
            assert!(
                m.min_d >= COLLIDE_TOLERANCE * 2.0 * radius,
                "{name} {settings:?}: min pair distance {} < {COLLIDE_TOLERANCE} * 2r",
                m.min_d
            );
            assert!(m.peak < 1000.0, "{name}: layout unbounded, peak {}", m.peak);
            let settle = m.settle.unwrap_or(usize::MAX);
            assert!(
                settle <= 300,
                "{name} {settings:?}: no tick under 1 unit before 300 ({:?})",
                m.settle
            );
            let mut sim = load(&s, configure);
            for _ in 0..600 {
                sim.tick();
            }
            assert!(sim.is_finished(), "{name}: still running after 600 ticks");
        }
    }
}

/// With the recommended settings, no pair of nodes is closer than 40 units on
/// any fixture, and the layout is no larger than 1.25x the default one.
#[test]
fn recommended_settings_clear_every_close_pair_without_inflating_the_layout() {
    for (name, json) in FIXTURES {
        let s = scope(json);
        let (_, base) = run(&s, 600, |_| {});
        let (_, m) = run(&s, 600, |sim| {
            for &(k, v) in RECOMMENDED {
                sim.set_param(k, v);
            }
        });
        assert_eq!(m.close40, 0, "{name}: {} pairs closer than 40", m.close40);
        assert!(
            m.peak <= 1.25 * base.peak,
            "{name}: peak {} vs {} with defaults",
            m.peak,
            base.peak
        );
    }
}

#[test]
fn component_gravity_packs_components_around_the_main_one() {
    for (name, json) in FIXTURES {
        let s = scope(json);
        let comps = components(&s);
        let (_, base) = run(&s, 600, |_| {});
        let (p, m) = run(&s, 600, |sim| sim.set_param("componentGravity", 0.1));
        let n = s.n();
        let (gx, gy) = centroid(&p, 0..n);
        let dist = |i: usize| (p[i * 2] - gx).hypot(p[i * 2 + 1] - gy);
        let main = comps.iter().max_by_key(|c| c.len()).expect("a component");
        // The main component's reach from the overall centroid.
        let reach = main.iter().map(|&i| dist(i)).fold(0.0f32, f32::max);
        for c in &comps {
            let (cx, cy) = centroid(&p, c.iter().copied());
            let own = c
                .iter()
                .map(|&i| (p[i * 2] - cx).hypot(p[i * 2 + 1] - cy))
                .fold(0.0f32, f32::max);
            let off = (cx - gx).hypot(cy - gy);
            let bound = reach + own + LINK_DISTANCE;
            assert!(
                off <= bound,
                "{name}: a {}-node component sits {off:.1} from the centroid, bound {bound:.1}",
                c.len()
            );
        }
        if comps.len() > 1 {
            assert!(
                m.comp_off < 0.75 * base.comp_off,
                "{name}: component offset {:.1} vs {:.1} with defaults",
                m.comp_off,
                base.comp_off
            );
        }
    }
}
