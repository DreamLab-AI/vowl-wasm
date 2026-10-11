//! Migration test for ontomark step 3 (`ontomark/taxonomy`), EXP-10 row VW-03.
//!
//! Red until the hard-coded six-domain `Domain` enum (with `from_prefix`,
//! `prefix` and `base_uri` and their `narrativegoldmine.com` base URIs) is
//! deleted from `src/ontology/mod.rs` and the manifest depends on an ontomark
//! crate for domain and taxonomy tables (`profiles/estate`, read through
//! `ontomark-model`). Reads files only; never imports ontomark.

use std::fs;
use std::path::PathBuf;

fn read(rel: &str) -> String {
    fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)).unwrap_or_default()
}

fn dependencies_section() -> String {
    let mut out = String::new();
    let mut inside = false;
    for line in read("Cargo.toml").lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            inside = t == "[dependencies]";
            continue;
        }
        if inside {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

#[test]
fn vw03_manifest_depends_on_an_ontomark_crate() {
    let has = dependencies_section().lines().any(|l| {
        l.trim_start().starts_with("ontomark-model")
            || l.trim_start().starts_with("ontomark-compile")
            || l.trim_start().starts_with("ontomark-ngg1")
    });
    assert!(
        has,
        "VW-03: Cargo.toml [dependencies] names no ontomark crate (`ontomark-model`, `ontomark-compile` or `ontomark-ngg1`) for domain and taxonomy tables"
    );
}

#[test]
fn vw03_domain_enum_and_base_uris_are_gone() {
    let mut found = Vec::new();
    for f in [
        "src/ontology/mod.rs",
        "src/ontology/model.rs",
        "src/ontology/parser.rs",
        "src/ontology/loader.rs",
        "src/ontology/markdown_parser.rs",
        "src/bindings/mod.rs",
        "src/lib.rs",
    ] {
        let s = read(f);
        for (sym, what) in [
            ("enum Domain", "`Domain` enum (src/ontology/mod.rs:216)"),
            ("fn from_prefix", "`Domain::from_prefix` (:244)"),
            ("fn base_uri", "`Domain::base_uri` (:269)"),
            (
                "http://narrativegoldmine.com/",
                "hard-coded domain base URI",
            ),
        ] {
            if s.contains(sym) {
                found.push(format!("VW-03: {what} still present in {f}"));
            }
        }
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
}
