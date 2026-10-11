//! Migration test for ontomark step 5 (`ontomark/ngg1`), EXP-10 rows VW-01, VW-02, VW-03, VW-04.
//!
//! Red until `src/ngg1.rs` is a re-export of ontomark's NGG1 codec and the
//! `markdown-ontology` feature (decision D8) is deleted. It reads files and the
//! manifest only; it never imports ontomark, so it compiles with or without it.
//!
//! Bundle weight: the published bundle is 232,052 bytes (README), built with
//! `--features ngg1,interaction`. The re-export must not pull a reasoner, an RDF
//! stack, a YAML parser or a regex engine into it. The dependency assertions
//! below check the manifest and `Cargo.lock`; the byte check at the end runs
//! when a `pkg/vowl_wasm_bg.wasm` built with `wasm-pack build --release
//! --target web -- --no-default-features --features ngg1,interaction` exists.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const PUBLISHED_BUNDLE_BYTES: u64 = 232_052;
/// Head-room over the published size; the re-export replaces the hand-written
/// reader, so the bundle must stay near 232,052 bytes.
const BUNDLE_BUDGET_BYTES: u64 = PUBLISHED_BUNDLE_BYTES + PUBLISHED_BUNDLE_BYTES / 20;

/// The wasm_bindgen surface listed in `pkg/vowl_wasm.d.ts` for 0.1.2
/// (`--features ngg1,interaction`). Frozen: `NggExplorer`.
const NGG_EXPLORER_JS: &[&str] = &[
    "edgeCount",
    "getAlpha",
    "isFinished",
    "loadCsr",
    "nodeCount",
    "positionsLen",
    "positionsPtr",
    "reheat",
    "setParam",
    "tick",
    "writePositionsInto",
];

/// Frozen: `WebVowl`.
const WEB_VOWL_JS: &[&str] = &[
    "checkNodeClick",
    "filterByDegree",
    "filterHierarchy",
    "getAlpha",
    "getEdgeCount",
    "getMetadata",
    "getNodeCount",
    "getNodeDegree",
    "getNodeIds",
    "getNodePositions",
    "getPinnedCount",
    "getStatistics",
    "initSimulation",
    "isFinished",
    "isPinned",
    "loadOntology",
    "pinNode",
    "resetPins",
    "resetVisibility",
    "runSimulation",
    "setCenter",
    "setChargeStrength",
    "setLinkDistance",
    "tick",
    "toggleDatatypes",
    "toggleSetOperators",
    "unpinNode",
    "writePositionsInto",
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    fs::read_to_string(root().join(rel)).unwrap_or_default()
}

fn exists(rel: &str) -> bool {
    root().join(rel).exists()
}

/// Text of one `[section]` of Cargo.toml, up to the next table header.
fn manifest_section(name: &str) -> String {
    let manifest = read("Cargo.toml");
    let header = format!("[{name}]");
    let mut out = String::new();
    let mut inside = false;
    for line in manifest.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            inside = t == header;
            continue;
        }
        if inside {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// The dependency line (with continuation lines) for `name` in `[dependencies]`.
fn dependency_entry(name: &str) -> Option<String> {
    let deps = manifest_section("dependencies");
    let mut found: Option<String> = None;
    for line in deps.lines() {
        let t = line.trim();
        if t.starts_with(name) && t[name.len()..].trim_start().starts_with('=') {
            found = Some(t.to_string());
        }
    }
    found
}

/// Package names in `Cargo.lock` (optional dependencies are locked too).
fn locked_packages() -> BTreeSet<String> {
    read("Cargo.lock")
        .split("[[package]]")
        .filter_map(|p| p.lines().find_map(|l| l.strip_prefix("name = ")))
        .map(|n| n.trim_matches('"').to_string())
        .collect()
}

fn camel(snake: &str) -> String {
    let mut out = String::new();
    let mut up = false;
    for c in snake.chars() {
        if c == '_' {
            up = true;
        } else if up {
            out.extend(c.to_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// Every JS-visible name declared under `src/bindings/` and `src/lib.rs`:
/// explicit `js_name = x` plus the camel-cased name of every `pub fn`.
fn exported_js_names(files: &[&str]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for f in files {
        for line in read(f).lines() {
            let t = line.trim();
            if let Some(rest) = t.split("js_name = ").nth(1) {
                let n: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                names.insert(n);
            }
            if let Some(rest) = t.strip_prefix("pub fn ") {
                let n: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                names.insert(camel(&n));
            }
        }
    }
    names
}

#[test]
fn vw01_manifest_depends_on_the_ontomark_ngg1_codec() {
    // Decided (queen, 2026-10-11): NGG1 has its own dependency-light crate,
    // `ontomark-ngg1`, and vowl-wasm depends on it directly. Depending on
    // ontomark-compile (even feature-gated) is not accepted.
    let ngg_crate = dependency_entry("ontomark-ngg1");
    let compile = dependency_entry("ontomark-compile");
    assert!(
        ngg_crate.is_some() && compile.is_none(),
        "VW-01: Cargo.toml [dependencies] must name `ontomark-ngg1` and not `ontomark-compile` \
         (found ngg1 crate: {ngg_crate:?}, compile: {compile:?})"
    );
}

#[test]
fn vw01_ngg1_rs_carries_no_format_constants_of_its_own() {
    let src = read("src/ngg1.rs");
    assert!(
        src.contains("pub use ontomark"),
        "VW-01: src/ngg1.rs must `pub use` the ontomark NGG1 codec (no `pub use ontomark…` found)"
    );
    let mut offenders = Vec::new();
    for (i, line) in src.lines().enumerate() {
        let t = line.trim();
        let is_const = (t.starts_with("pub const ") || t.starts_with("const ")) && t.contains(':');
        let is_decoder =
            t.contains("from_le_bytes") || t.contains("0x3147_474E") || t.contains("0x3147474E");
        let is_parse = t.contains("fn parse(");
        if is_const || is_decoder || is_parse {
            offenders.push(format!("src/ngg1.rs:{}: {t}", i + 1));
        }
    }
    assert!(
        offenders.is_empty(),
        "VW-01: src/ngg1.rs still holds a second NGG1 decoder or format constants:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn vw02_vw04_d8_markdown_ontology_feature_and_files_are_gone() {
    let mut present = Vec::new();
    for (row, f) in [
        ("VW-02", "src/ontology/markdown_parser.rs"),
        ("VW-02", "tests/markdown_parser_test.rs"),
        ("VW-04", "src/ontology/owl2_validator.rs"),
        ("VW-04", "tests/owl2_validation_test.rs"),
        // VW-07 lists loader.rs as RETAIN, but it is gated by `markdown-ontology`
        // and is built on the markdown parser and `Domain`, so D8 takes it too.
        ("VW-07/D8", "src/ontology/loader.rs"),
    ] {
        if exists(f) {
            present.push(format!("{row}: {f}"));
        }
    }
    assert!(
        present.is_empty(),
        "D8: files still present:\n{}",
        present.join("\n")
    );

    let locked = locked_packages();
    for pkg in ["regex", "aho-corasick"] {
        assert!(
            !locked.contains(pkg),
            "D8: `{pkg}` is still in Cargo.lock; only markdown-ontology needed it, and the ontomark NGG1 re-export must not bring it back"
        );
    }
    let manifest = read("Cargo.toml");
    assert!(
        !manifest.contains("markdown-ontology"),
        "D8: Cargo.toml still mentions the `markdown-ontology` feature (feature, [[test]] required-features)"
    );
    assert!(
        dependency_entry("regex").is_none(),
        "D8: `regex` (only needed by markdown-ontology) is still a dependency"
    );
    for f in ["src/bindings/mod.rs", "src/ontology/mod.rs", "src/lib.rs"] {
        let s = read(f);
        for sym in [
            "markdown_parser",
            "owl2_validator",
            "parseMarkdownOntology",
            "validateOWL2",
            "markdown-ontology",
        ] {
            assert!(
                !s.contains(sym),
                "D8 (VW-02/VW-04): `{sym}` still referenced in {f}"
            );
        }
    }
}

#[test]
fn vw03_domain_enum_is_gone() {
    for f in [
        "src/ontology/mod.rs",
        "src/ontology/model.rs",
        "src/ontology/parser.rs",
        "src/lib.rs",
    ] {
        let s = read(f);
        assert!(
            !s.contains("enum Domain"),
            "VW-03: `enum Domain` still declared in {f}"
        );
        assert!(
            !s.contains("fn from_prefix"),
            "VW-03: `Domain::from_prefix` still in {f}"
        );
        assert!(
            !s.contains("http://narrativegoldmine.com/"),
            "VW-03: hard-coded domain base URIs still in {f}"
        );
    }
}

#[test]
fn js_surface_is_frozen() {
    let explorer = exported_js_names(&["src/bindings/explorer.rs"]);
    for n in NGG_EXPLORER_JS {
        assert!(
            explorer.contains(*n),
            "frozen JS API: NggExplorer.{n} missing from src/bindings/explorer.rs"
        );
    }
    let web = exported_js_names(&["src/bindings/mod.rs"]);
    for n in WEB_VOWL_JS {
        assert!(
            web.contains(*n),
            "frozen JS API: WebVowl.{n} missing from src/bindings/mod.rs"
        );
    }
    let top = exported_js_names(&["src/lib.rs"]);
    for n in ["init", "version"] {
        assert!(
            top.contains(n),
            "frozen JS API: {n} missing from src/lib.rs"
        );
    }
    // When a built package is present, its .d.ts must still list every frozen name.
    let dts = read("pkg/vowl_wasm.d.ts");
    if !dts.is_empty() {
        for n in NGG_EXPLORER_JS.iter().chain(WEB_VOWL_JS) {
            assert!(
                dts.contains(&format!("{n}(")),
                "frozen JS API: pkg/vowl_wasm.d.ts lost `{n}`"
            );
        }
        for gone in ["parseMarkdownOntology", "validateOWL2"] {
            assert!(
                !dts.contains(gone),
                "pkg/vowl_wasm.d.ts must not export `{gone}` (never in the published bundle)"
            );
        }
    }
}

#[test]
fn bundle_does_not_gain_reasoner_rdf_yaml_or_regex_dependencies() {
    // `regex` and `aho-corasick` are checked against Cargo.lock in the D8 test (they are
    // locked today only because `markdown-ontology` is declared); the rest must be absent
    // from the manifest and the lock at all times.
    const FORBIDDEN: &[&str] = &[
        "whelk-el",
        "ontomark-reason",
        "ontomark-parse",
        "serde_yaml",
        "oxrdf",
        "oxttl",
        "oxigraph",
        "sophia_api",
        "rio_turtle",
    ];
    let deps = manifest_section("dependencies");
    let locked = locked_packages();
    let mut bad = Vec::new();
    for f in FORBIDDEN {
        let direct = deps.lines().any(|l| l.trim_start().starts_with(f));
        if direct || locked.contains(*f) {
            bad.push(*f);
        }
    }
    assert!(
        bad.is_empty(),
        "bundle budget (232,052 B published, README): the NGG1 re-export pulled in {bad:?}; \
         depend on `ontomark-ngg1`, which carries no reasoner, RDF or YAML crates"
    );
    // Opt-in byte check against a freshly built bundle.
    let wasm: &Path = &root().join("pkg/vowl_wasm_bg.wasm");
    if let Ok(meta) = fs::metadata(wasm) {
        assert!(
            meta.len() <= BUNDLE_BUDGET_BYTES,
            "published-bundle check: pkg/vowl_wasm_bg.wasm is {} B, budget {BUNDLE_BUDGET_BYTES} B (published {PUBLISHED_BUNDLE_BYTES} B)",
            meta.len()
        );
    }
}
