//! Migration test for ontomark step 5 (`ontomark/ngg1`), EXP-10 rows VW-01, VW-02, VW-04
//! (and VW-07/D8 for `loader.rs`).
//!
//! Green only when `src/ngg1.rs` is a re-export of the `ontomark-ngg1` crate and the
//! `markdown-ontology` feature (decision D8) and everything behind it is gone. VW-03 (the
//! `Domain` enum) belongs to step 3 and is asserted by the `ontomark/taxonomy` branch.
//!
//! How the checks read the repository (nothing here is a substring search):
//!
//! * Manifests: `cargo metadata` output, which is cargo's own parse of `Cargo.toml`. A
//!   dependency counts by its real package name (a `package = "..."` rename cannot pass for
//!   another crate), its kind (`null` = normal), its registry source and its `optional` flag. The
//!   `ngg1` feature must switch it on. Comments and `[package.metadata]` cannot satisfy it.
//! * Rust source: parsed with `syn`, so comments never count, and strings and numbers are
//!   literals, not text. The tests directory is never scanned (a test does not scan itself).
//! * Bundle weight: the *normal* dependency graph of `vowl-wasm` for
//!   `--features ngg1,interaction` on `wasm32-unknown-unknown` (what the published bundle
//!   links), not `Cargo.lock`. Dev-dependencies such as `criterion` (which brings `regex`)
//!   do not ship and are ignored. The built artefact is checked when `pkg/` exists, and the
//!   check is mandatory when `VOWL_BUNDLE_GATE=1` (CI sets it after `wasm-pack build`).
//!
//! Measured on `main` (wasm-pack 0.15, wasm-bindgen 0.2.128, `wasm-pack build --release
//! --target web --features ngg1,interaction`): 232,764 bytes raw, 91,418 gzip -9.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use syn::visit::Visit;

/// Raw size of `pkg/vowl_wasm_bg.wasm` built from `main` (see the module docs).
const MAIN_BUNDLE_BYTES: u64 = 232_764;
/// Head-room over `main`; the re-export replaces a hand-written reader, so the bundle must not grow.
const BUNDLE_BUDGET_BYTES: u64 = MAIN_BUNDLE_BYTES + MAIN_BUNDLE_BYTES / 20;
/// A real vowl-wasm module is hundreds of kilobytes; anything below this is a stub.
const BUNDLE_FLOOR_BYTES: u64 = 100_000;

/// The exact wasm-bindgen export surface of the published bundle
/// (`--features ngg1,interaction`), as listed in `pkg/vowl_wasm.d.ts` built from `main`
/// (the 0.1.2 API). `parseMarkdownOntology`, `validateOWL2` and `getGraphData` are absent from
/// it by history: the first two were behind `markdown-ontology` and are removed on purpose
/// (D8); `getGraphData` is `debug-serde` only.
const FROZEN_JS_EXPORTS: &[&str] = &[
    "NggExplorer.constructor",
    "NggExplorer.edgeCount",
    "NggExplorer.getAlpha",
    "NggExplorer.isFinished",
    "NggExplorer.loadCsr",
    "NggExplorer.nodeCount",
    "NggExplorer.positionsLen",
    "NggExplorer.positionsPtr",
    "NggExplorer.reheat",
    "NggExplorer.setParam",
    "NggExplorer.tick",
    "NggExplorer.writePositionsInto",
    "WebVowl.checkNodeClick",
    "WebVowl.constructor",
    "WebVowl.filterByDegree",
    "WebVowl.filterHierarchy",
    "WebVowl.getAlpha",
    "WebVowl.getEdgeCount",
    "WebVowl.getMetadata",
    "WebVowl.getNodeCount",
    "WebVowl.getNodeDegree",
    "WebVowl.getNodeIds",
    "WebVowl.getNodePositions",
    "WebVowl.getPinnedCount",
    "WebVowl.getStatistics",
    "WebVowl.initSimulation",
    "WebVowl.isFinished",
    "WebVowl.isPinned",
    "WebVowl.loadOntology",
    "WebVowl.pinNode",
    "WebVowl.resetPins",
    "WebVowl.resetVisibility",
    "WebVowl.runSimulation",
    "WebVowl.setCenter",
    "WebVowl.setChargeStrength",
    "WebVowl.setLinkDistance",
    "WebVowl.tick",
    "WebVowl.toggleDatatypes",
    "WebVowl.toggleSetOperators",
    "WebVowl.unpinNode",
    "WebVowl.writePositionsInto",
    "class NggExplorer",
    "class WebVowl",
    "fn init",
    "fn version",
];

/// Features the published bundle is built with.
const BUNDLE_FEATURES: &[&str] = &["ngg1", "interaction"];

/// Items the D8 deletion removed (parser, validator, loader). Their names must not be defined
/// anywhere outside `tests/`, so moving the code to another path does not count as removal.
const D8_DEFINING_NAMES: &[&str] = &[
    "MarkdownParser",
    "OWL2Validator",
    "ValidationResult",
    "LoaderConfig",
    "LoaderStatistics",
    "OntologyLoader",
    "build_full_iri",
    "extract_ontology_block",
    "extract_all_properties",
    "extract_owl_axioms",
    "separate_properties",
    "parse_owl_class",
    "validate_iri_format",
    "validate_namespace",
    "check_owl2_dl_compliance",
    "detect_antipatterns",
    "check_iri_uniqueness",
    "validate_block",
    "validate_blocks",
    "load_directory",
    "filter_by_domain",
    "group_by_domain",
    "domain_summary",
    "create_term_index",
    "collect_files_recursive",
    "load_files_parallel",
    "is_pascal_case",
    "is_term_id",
    "is_uri_scheme",
    "parseMarkdownOntology",
    "validateOWL2",
    "parse_markdown_ontology",
    "validate_owl2",
];

/// String literals that identify the deleted behaviour even if every name is changed.
const D8_BEHAVIOUR_MARKERS: &[&str] = &[
    "### OntologyBlock",
    "#### OWL Axioms",
    "OntologyBlock section not found",
    "IRI cannot contain spaces",
    "Duplicate IRI found",
    "should start with uppercase (PascalCase)",
    "Antipattern:",
    "markdown-ontology",
    "parseMarkdownOntology",
    "validateOWL2",
];

/// Names defined by the hand-written NGG1 reader on `main` (`src/ngg1.rs`).
const NGG1_DEFINING_NAMES: &[&str] = &[
    "NGG1_MAGIC",
    "NGG1_VERSION",
    "NGG1_HEADER_SIZE",
    "NGG1_NODE_STRIDE",
    "EDGE_SUBCLASS",
    "EDGE_RELATION",
    "FLAG_DOMAIN_ROOT",
    "FLAG_CATEGORY_ROOT",
    "FLAG_HAS_PAGE",
    "FLAG_BRIDGE",
    "CATEGORY_NONE",
    "Ngg1",
    "Ngg1Error",
    "NodeRecord",
    "OkOrOv",
    "csr_topology",
    "seed_positions_packed",
    "rd_u16",
    "rd_u32",
    "rd_f32",
];

/// Files that may exist in `src/ontology/` once D8 has landed. A new file here is how a
/// "removed" module would reappear under a new name.
const ONTOLOGY_FILES: &[&str] = &["mod.rs", "model.rs", "parser.rs"];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn exists(rel: &str) -> bool {
    root().join(rel).exists()
}

fn read_required(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

// ---------------------------------------------------------------------------------------------
// Manifest, through cargo metadata
// ---------------------------------------------------------------------------------------------

fn metadata(extra: &[&str]) -> Value {
    let out = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--offline",
            "--manifest-path",
        ])
        .arg(root().join("Cargo.toml"))
        .args(extra)
        .output()
        .expect("cannot run cargo metadata");
    assert!(
        out.status.success(),
        "cargo metadata failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("cargo metadata output is not JSON")
}

/// The `vowl-wasm` package entry of a `cargo metadata --no-deps` run.
fn own_package() -> Value {
    let meta = metadata(&["--no-deps"]);
    let pkgs = meta["packages"].as_array().expect("packages");
    assert_eq!(pkgs.len(), 1, "expected exactly the vowl-wasm package");
    assert_eq!(pkgs[0]["name"], "vowl-wasm");
    pkgs[0].clone()
}

fn crates_io(source: &Value) -> bool {
    source
        .as_str()
        .is_some_and(|s| s == "registry+https://github.com/rust-lang/crates.io-index")
}

/// The one `ontomark-ngg1` dependency, validated, returning the key it is known by in Cargo.toml.
fn ngg1_dependency_key(pkg: &Value) -> String {
    let deps = pkg["dependencies"].as_array().expect("dependencies");
    let ngg: Vec<&Value> = deps
        .iter()
        .filter(|d| d["name"] == "ontomark-ngg1")
        .collect();
    assert_eq!(
        ngg.len(),
        1,
        "VW-01: expected exactly one dependency whose package is `ontomark-ngg1`, found {}",
        ngg.len()
    );
    let d = ngg[0];
    assert!(
        d["kind"].is_null(),
        "VW-01: `ontomark-ngg1` must be a normal [dependencies] entry, not {:?}",
        d["kind"]
    );
    assert!(
        d["target"].is_null(),
        "VW-01: `ontomark-ngg1` must not be target-specific (the bundle is wasm32)"
    );
    assert_eq!(
        d["optional"], true,
        "VW-01: `ontomark-ngg1` must be optional (default builds do not carry NGG1)"
    );
    assert!(
        crates_io(&d["source"]),
        "VW-01: `ontomark-ngg1` must come from crates.io, source was {:?}",
        d["source"]
    );
    let req = d["req"].as_str().unwrap_or("");
    assert!(
        !req.is_empty() && req != "*",
        "VW-01: `ontomark-ngg1` needs a real version requirement, got {req:?}"
    );
    for other in deps {
        let name = other["name"].as_str().unwrap_or("");
        assert!(
            name == "ontomark-ngg1" || !name.starts_with("ontomark"),
            "VW-01: `{name}` is not accepted; NGG1 depends on `ontomark-ngg1` only \
             (ontomark-compile carries serde_yaml and regex)"
        );
    }
    d["rename"].as_str().unwrap_or("ontomark-ngg1").to_string()
}

#[test]
fn vw01_manifest_depends_on_the_ontomark_ngg1_crate_through_the_ngg1_feature() {
    let pkg = own_package();
    let key = ngg1_dependency_key(&pkg);
    let feature = pkg["features"]["ngg1"]
        .as_array()
        .expect("VW-01: the `ngg1` feature must exist");
    let wires = feature
        .iter()
        .any(|f| f.as_str() == Some(&format!("dep:{key}")));
    assert!(
        wires,
        "VW-01: feature `ngg1` must enable `dep:{key}`, it lists {feature:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// Rust source, through syn
// ---------------------------------------------------------------------------------------------

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}

/// Every shipped or tooling Rust file: `src`, `benches`, `examples`, `build.rs`. Never `tests`.
fn non_test_sources() -> Vec<PathBuf> {
    let mut files = Vec::new();
    for d in ["src", "benches", "examples"] {
        rust_files(&root().join(d), &mut files);
    }
    let build = root().join("build.rs");
    if build.exists() {
        files.push(build);
    }
    assert!(
        files.iter().any(|f| f.ends_with("src/lib.rs")),
        "src/lib.rs is missing"
    );
    files
}

fn parse(path: &Path) -> syn::File {
    syn::parse_file(&read_required(path))
        .unwrap_or_else(|e| panic!("cannot parse {}: {e}", path.display()))
}

/// Names defined by items, and literals, found anywhere in one parsed file.
#[derive(Default)]
struct Facts {
    defined: BTreeSet<String>,
    strings: Vec<String>,
    ints: BTreeSet<u128>,
    byte_strings: Vec<Vec<u8>>,
    /// `from_le_bytes`-style calls: how a binary format is read by hand.
    byte_decoders: BTreeSet<String>,
}

impl Facts {
    fn note_decoder(&mut self, ident: &syn::Ident) {
        let n = ident.to_string();
        if matches!(
            n.as_str(),
            "from_le_bytes" | "from_be_bytes" | "from_ne_bytes"
        ) {
            self.byte_decoders.insert(n);
        }
    }
}

impl<'ast> Visit<'ast> for Facts {
    fn visit_item_fn(&mut self, i: &'ast syn::ItemFn) {
        self.defined.insert(i.sig.ident.to_string());
        syn::visit::visit_item_fn(self, i);
    }
    fn visit_impl_item_fn(&mut self, i: &'ast syn::ImplItemFn) {
        self.defined.insert(i.sig.ident.to_string());
        syn::visit::visit_impl_item_fn(self, i);
    }
    fn visit_trait_item_fn(&mut self, i: &'ast syn::TraitItemFn) {
        self.defined.insert(i.sig.ident.to_string());
        syn::visit::visit_trait_item_fn(self, i);
    }
    fn visit_item_struct(&mut self, i: &'ast syn::ItemStruct) {
        self.defined.insert(i.ident.to_string());
        syn::visit::visit_item_struct(self, i);
    }
    fn visit_item_enum(&mut self, i: &'ast syn::ItemEnum) {
        self.defined.insert(i.ident.to_string());
        syn::visit::visit_item_enum(self, i);
    }
    fn visit_item_trait(&mut self, i: &'ast syn::ItemTrait) {
        self.defined.insert(i.ident.to_string());
        syn::visit::visit_item_trait(self, i);
    }
    fn visit_item_const(&mut self, i: &'ast syn::ItemConst) {
        self.defined.insert(i.ident.to_string());
        syn::visit::visit_item_const(self, i);
    }
    fn visit_impl_item_const(&mut self, i: &'ast syn::ImplItemConst) {
        self.defined.insert(i.ident.to_string());
        syn::visit::visit_impl_item_const(self, i);
    }
    fn visit_item_static(&mut self, i: &'ast syn::ItemStatic) {
        self.defined.insert(i.ident.to_string());
        syn::visit::visit_item_static(self, i);
    }
    fn visit_item_type(&mut self, i: &'ast syn::ItemType) {
        self.defined.insert(i.ident.to_string());
        syn::visit::visit_item_type(self, i);
    }
    fn visit_item_mod(&mut self, i: &'ast syn::ItemMod) {
        self.defined.insert(i.ident.to_string());
        syn::visit::visit_item_mod(self, i);
    }
    fn visit_expr_method_call(&mut self, i: &'ast syn::ExprMethodCall) {
        self.note_decoder(&i.method);
        syn::visit::visit_expr_method_call(self, i);
    }
    fn visit_expr_path(&mut self, i: &'ast syn::ExprPath) {
        if let Some(seg) = i.path.segments.last() {
            self.note_decoder(&seg.ident);
        }
        syn::visit::visit_expr_path(self, i);
    }
    fn visit_lit_str(&mut self, i: &'ast syn::LitStr) {
        self.strings.push(i.value());
    }
    fn visit_lit_int(&mut self, i: &'ast syn::LitInt) {
        if let Ok(v) = i.base10_parse::<u128>() {
            self.ints.insert(v);
        }
    }
    fn visit_lit_byte_str(&mut self, i: &'ast syn::LitByteStr) {
        self.byte_strings.push(i.value());
    }
}

fn facts_of(files: &[PathBuf]) -> BTreeMap<PathBuf, Facts> {
    files
        .iter()
        .map(|p| {
            let mut f = Facts::default();
            f.visit_file(&parse(p));
            (p.clone(), f)
        })
        .collect()
}

fn rel(p: &Path) -> String {
    p.strip_prefix(root()).unwrap_or(p).display().to_string()
}

/// The entire public codec must remain available through the old module path.
/// A named subset can compile internally while silently dropping consumer APIs.
fn full_codec_reexport(u: &syn::ItemUse, key: &str) -> bool {
    matches!(u.vis, syn::Visibility::Public(_))
        && matches!(&u.tree, syn::UseTree::Path(p)
            if p.ident == key && matches!(&*p.tree, syn::UseTree::Glob(_)))
}

#[test]
fn reexport_guard_rejects_partial_nested_and_renamed_exports() {
    for source in [
        "pub use ontomark_ngg1::{Ngg1, Ngg1Error};",
        "pub use ontomark_ngg1::Ngg1;",
        "pub use ontomark_ngg1::nested::*;",
        "pub use ontomark_ngg1::Ngg1 as Codec;",
        "use ontomark_ngg1::*;",
        "pub(crate) use ontomark_ngg1::*;",
        "pub use another_crate::*;",
    ] {
        let item: syn::ItemUse = syn::parse_str(source).unwrap();
        assert!(
            !full_codec_reexport(&item, "ontomark_ngg1"),
            "a partial or private export passed the full API guard: {source}"
        );
    }
    for (source, key) in [
        ("pub use ontomark_ngg1::*;", "ontomark_ngg1"),
        ("pub use renamed_codec::*;", "renamed_codec"),
    ] {
        let item: syn::ItemUse = syn::parse_str(source).unwrap();
        assert!(full_codec_reexport(&item, key));
    }
}

#[test]
fn vw01_ngg1_rs_is_a_bare_reexport_and_no_second_codec_exists() {
    let pkg = own_package();
    let key = ngg1_dependency_key(&pkg).replace('-', "_");

    let file = parse(&root().join("src/ngg1.rs"));
    assert_eq!(
        file.items.len(),
        1,
        "VW-01: src/ngg1.rs must hold exactly one item, the `pub use`; found {}",
        file.items.len()
    );
    match &file.items[0] {
        syn::Item::Use(u) => {
            assert!(
                full_codec_reexport(u, &key),
                "VW-01: src/ngg1.rs must `pub use {key}::*;` to preserve the entire public codec API"
            );
        }
        other => panic!(
            "VW-01: src/ngg1.rs must hold only `pub use`, found {}",
            quote_kind(other)
        ),
    }

    // `ngg1` is declared in lib.rs behind the feature that enables the dependency.
    let lib = parse(&root().join("src/lib.rs"));
    let declared = lib.items.iter().any(|i| match i {
        syn::Item::Mod(m) if m.ident == "ngg1" && m.content.is_none() => {
            matches!(m.vis, syn::Visibility::Public(_)) && cfg_features(&m.attrs).contains("ngg1")
        }
        _ => false,
    });
    assert!(
        declared,
        "VW-01: src/lib.rs must declare `#[cfg(feature = \"ngg1\")] pub mod ngg1;`"
    );

    // No other NGG1 codec anywhere outside tests: no format constants, no magic number.
    let facts = facts_of(&non_test_sources());
    // The magic as a little-endian u32 and as the byte-swapped value a renamed copy might use.
    let magics = [0x3147_474E_u128, 0x4E47_4731_u128];
    let mut offenders = Vec::new();
    for (path, f) in &facts {
        for n in NGG1_DEFINING_NAMES {
            if f.defined.contains(*n) {
                offenders.push(format!("{}: defines `{n}`", rel(path)));
            }
        }
        if magics.iter().any(|m| f.ints.contains(m)) {
            offenders.push(format!("{}: holds the NGG1 magic (0x3147474E)", rel(path)));
        }
        if !f.byte_decoders.is_empty() {
            offenders.push(format!(
                "{}: reads binary fields by hand ({:?}); the NGG1 codec is ontomark-ngg1's",
                rel(path),
                f.byte_decoders
            ));
        }
        if f.byte_strings.iter().any(|b| b.starts_with(b"NGG1")) {
            offenders.push(format!("{}: holds a b\"NGG1\" magic", rel(path)));
        }
    }
    assert!(
        offenders.is_empty(),
        "VW-01: a second NGG1 reader or its constants remain:\n{}",
        offenders.join("\n")
    );
}

fn quote_kind(i: &syn::Item) -> &'static str {
    match i {
        syn::Item::Const(_) => "a const",
        syn::Item::Fn(_) => "a fn",
        syn::Item::Struct(_) => "a struct",
        syn::Item::Enum(_) => "an enum",
        syn::Item::Impl(_) => "an impl",
        syn::Item::Mod(_) => "a mod",
        syn::Item::Static(_) => "a static",
        syn::Item::Trait(_) => "a trait",
        syn::Item::Type(_) => "a type alias",
        _ => "another item",
    }
}

/// Features named by the `cfg(...)` attributes in `attrs` (only positive occurrences matter here).
fn cfg_features(attrs: &[syn::Attribute]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for a in attrs.iter().filter(|a| a.path().is_ident("cfg")) {
        if let syn::Meta::List(l) = &a.meta {
            collect_features(&l.tokens, &mut out);
        }
    }
    out
}

fn collect_features(tokens: &proc_macro2::TokenStream, out: &mut BTreeSet<String>) {
    let mut it = tokens.clone().into_iter().peekable();
    while let Some(t) = it.next() {
        match t {
            proc_macro2::TokenTree::Ident(i) if i == "feature" => {
                if let (Some(_eq), Some(proc_macro2::TokenTree::Literal(l))) =
                    (it.next(), it.next())
                {
                    out.insert(l.to_string().trim_matches('"').to_string());
                }
            }
            proc_macro2::TokenTree::Group(g) => collect_features(&g.stream(), out),
            _ => {}
        }
    }
}

#[test]
fn vw02_vw04_d8_markdown_ontology_files_features_and_behaviour_are_gone() {
    // Exact files named by D8.
    let mut present = Vec::new();
    for (row, f) in [
        ("VW-02", "src/ontology/markdown_parser.rs"),
        ("VW-02", "tests/markdown_parser_test.rs"),
        ("VW-04", "src/ontology/owl2_validator.rs"),
        ("VW-04", "tests/owl2_validation_test.rs"),
        // VW-07 lists loader.rs as RETAIN, but it is gated by `markdown-ontology` and is
        // built on the markdown parser and `Domain`, so D8 takes it too (DEPRECATION 9.1).
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

    // Manifest: no feature, no test target, no direct regex dependency.
    let pkg = own_package();
    let features = pkg["features"].as_object().expect("features");
    assert!(
        !features.contains_key("markdown-ontology"),
        "D8: the `markdown-ontology` feature is still declared"
    );
    for (name, list) in features {
        let refers = list
            .as_array()
            .into_iter()
            .flatten()
            .any(|v| v.as_str().is_some_and(|s| s.contains("markdown-ontology")));
        assert!(
            !refers,
            "D8: feature `{name}` still refers to markdown-ontology"
        );
    }
    for t in pkg["targets"].as_array().expect("targets") {
        let req = t["required-features"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        assert!(
            !req.iter().any(|f| f == "markdown-ontology"),
            "D8: target {} still requires markdown-ontology",
            t["name"]
        );
    }
    let regex_dep = pkg["dependencies"]
        .as_array()
        .expect("dependencies")
        .iter()
        .any(|d| d["name"] == "regex" && d["kind"].is_null());
    assert!(
        !regex_dep,
        "D8: `regex` (only needed by markdown-ontology) is still a [dependencies] entry"
    );

    // Source: deleted definitions and behaviour are not anywhere under a new name or path.
    let files = non_test_sources();
    let facts = facts_of(&files);
    let mut offenders = Vec::new();
    for (path, f) in &facts {
        for n in D8_DEFINING_NAMES {
            if f.defined.contains(*n) {
                offenders.push(format!("{}: defines `{n}`", rel(path)));
            }
        }
        for s in &f.strings {
            for m in D8_BEHAVIOUR_MARKERS {
                if s.contains(m) {
                    offenders.push(format!("{}: string literal carries `{m}`", rel(path)));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "D8: parser, validator or loader logic is still in the tree:\n{}",
        offenders.join("\n")
    );

    // Module declarations across the whole module tree.
    let mut mods = BTreeSet::new();
    for path in &files {
        for item in parse(path).items {
            if let syn::Item::Mod(m) = item {
                mods.insert(m.ident.to_string());
            }
        }
    }
    for gone in ["markdown_parser", "owl2_validator", "loader"] {
        assert!(!mods.contains(gone), "D8: `mod {gone}` is still declared");
    }

    // Nothing new in src/ontology (a rename would land there).
    let mut found = BTreeSet::new();
    for e in fs::read_dir(root().join("src/ontology"))
        .expect("src/ontology")
        .flatten()
    {
        found.insert(e.file_name().to_string_lossy().to_string());
    }
    let allowed: BTreeSet<String> = ONTOLOGY_FILES.iter().map(|s| s.to_string()).collect();
    assert_eq!(
        found, allowed,
        "D8: src/ontology must hold exactly {ONTOLOGY_FILES:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// Frozen JavaScript API
// ---------------------------------------------------------------------------------------------

fn cfg_active(attrs: &[syn::Attribute]) -> bool {
    attrs
        .iter()
        .filter(|a| a.path().is_ident("cfg"))
        .all(|a| match &a.meta {
            syn::Meta::List(l) => {
                let m: syn::Meta = syn::parse2(l.tokens.clone()).expect("cfg predicate");
                eval_cfg(&m)
            }
            _ => true,
        })
}

fn eval_cfg(m: &syn::Meta) -> bool {
    match m {
        syn::Meta::NameValue(nv) if nv.path.is_ident("feature") => match &nv.value {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) => BUNDLE_FEATURES.contains(&s.value().as_str()),
            _ => false,
        },
        syn::Meta::NameValue(nv) if nv.path.is_ident("target_arch") => true,
        syn::Meta::NameValue(_) => true,
        syn::Meta::Path(p) if p.is_ident("test") || p.is_ident("docsrs") => false,
        syn::Meta::Path(_) => true,
        syn::Meta::List(l) => {
            let args = l
                .parse_args_with(
                    syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
                )
                .expect("cfg arguments");
            if l.path.is_ident("all") {
                args.iter().all(eval_cfg)
            } else if l.path.is_ident("any") {
                args.iter().any(eval_cfg)
            } else if l.path.is_ident("not") {
                !args.iter().all(eval_cfg)
            } else {
                true
            }
        }
    }
}

/// The `wasm_bindgen` attribute on an item: `None` if absent, else its `js_name` and flags.
struct Bindgen {
    js_name: Option<String>,
    constructor: bool,
}

fn bindgen_attr(attrs: &[syn::Attribute]) -> Option<Bindgen> {
    let a = attrs.iter().find(|a| a.path().is_ident("wasm_bindgen"))?;
    let mut b = Bindgen {
        js_name: None,
        constructor: false,
    };
    if let syn::Meta::List(_) = &a.meta {
        a.parse_nested_meta(|m| {
            if m.path.is_ident("constructor") {
                b.constructor = true;
            } else if m.input.peek(syn::Token![=]) {
                let e: syn::Expr = m.value()?.parse()?;
                if m.path.is_ident("js_name") {
                    b.js_name = match e {
                        syn::Expr::Path(p) => p.path.get_ident().map(|i| i.to_string()),
                        syn::Expr::Lit(syn::ExprLit {
                            lit: syn::Lit::Str(s),
                            ..
                        }) => Some(s.value()),
                        _ => None,
                    };
                }
            }
            Ok(())
        })
        .expect("wasm_bindgen attribute");
    }
    Some(b)
}

fn module_file(dir: &Path, name: &str) -> PathBuf {
    let flat = dir.join(format!("{name}.rs"));
    if flat.exists() {
        flat
    } else {
        dir.join(name).join("mod.rs")
    }
}

/// wasm-bindgen exports reachable from `file` for the bundle features.
fn exports_of(file: &Path, dir: &Path, out: &mut BTreeSet<String>) {
    exports_in_items(parse(file).items, dir, out);
}

fn exports_in_items(items: Vec<syn::Item>, dir: &Path, out: &mut BTreeSet<String>) {
    for item in items {
        match item {
            syn::Item::Mod(m) if cfg_active(&m.attrs) => {
                let child_dir = dir.join(m.ident.to_string());
                match m.content {
                    Some((_, inner)) => exports_in_items(inner, &child_dir, out),
                    None => {
                        let f = module_file(dir, &m.ident.to_string());
                        let next_dir = if f.ends_with("mod.rs") {
                            f.parent().unwrap().to_path_buf()
                        } else {
                            child_dir
                        };
                        exports_of(&f, &next_dir, out);
                    }
                }
            }
            syn::Item::Struct(s) if cfg_active(&s.attrs) && bindgen_attr(&s.attrs).is_some() => {
                out.insert(format!("class {}", s.ident));
            }
            syn::Item::Fn(f) if cfg_active(&f.attrs) => {
                if let Some(b) = bindgen_attr(&f.attrs) {
                    let n = b.js_name.unwrap_or_else(|| f.sig.ident.to_string());
                    out.insert(format!("fn {n}"));
                }
            }
            syn::Item::Impl(i) if cfg_active(&i.attrs) && bindgen_attr(&i.attrs).is_some() => {
                let class = match &*i.self_ty {
                    syn::Type::Path(p) => p.path.segments.last().unwrap().ident.to_string(),
                    _ => panic!("wasm_bindgen impl on a non-path type"),
                };
                for it in i.items {
                    if let syn::ImplItem::Fn(f) = it {
                        if !cfg_active(&f.attrs) || !matches!(f.vis, syn::Visibility::Public(_)) {
                            continue;
                        }
                        let b = bindgen_attr(&f.attrs);
                        let name = match &b {
                            Some(b) if b.constructor => "constructor".to_string(),
                            Some(Bindgen {
                                js_name: Some(n), ..
                            }) => n.clone(),
                            _ => f.sig.ident.to_string(),
                        };
                        out.insert(format!("{class}.{name}"));
                    }
                }
            }
            other => {
                // Any other exported construct would widen the frozen surface.
                let attrs = match &other {
                    syn::Item::Enum(e) => &e.attrs,
                    syn::Item::Const(c) => &c.attrs,
                    syn::Item::Static(s) => &s.attrs,
                    syn::Item::ForeignMod(f) => &f.attrs,
                    syn::Item::Type(t) => &t.attrs,
                    _ => continue,
                };
                if cfg_active(attrs) && bindgen_attr(attrs).is_some() {
                    out.insert(format!("other {}", quote_kind(&other)));
                }
            }
        }
    }
}

/// Exported names listed by a wasm-bindgen `.d.ts`: classes, their members and free functions.
fn exports_of_dts(dts: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut class: Option<String> = None;
    for line in dts.lines() {
        if let Some(rest) = line.strip_prefix("export class ") {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            out.insert(format!("class {name}"));
            class = Some(name);
        } else if let Some(rest) = line.strip_prefix("export function ") {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if name != "initSync" {
                out.insert(format!("fn {name}"));
            }
            class = None;
        } else if line.starts_with('}') {
            class = None;
        } else if let (Some(c), true) = (
            &class,
            line.starts_with("    ") && !line.starts_with("     "),
        ) {
            let t = line.trim();
            let name: String = t
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() && t[name.len()..].starts_with('(') && name != "free" {
                out.insert(format!("{c}.{name}"));
            }
        }
    }
    out
}

fn frozen() -> BTreeSet<String> {
    FROZEN_JS_EXPORTS.iter().map(|s| s.to_string()).collect()
}

#[test]
fn js_surface_is_frozen_exactly() {
    let mut got = BTreeSet::new();
    exports_of(&root().join("src/lib.rs"), &root().join("src"), &mut got);
    assert_eq!(
        got,
        frozen(),
        "frozen JS API: the wasm_bindgen exports of src/ (features ngg1,interaction) differ from \
         the recorded 0.1.2 surface.\nadded: {:?}\nremoved: {:?}",
        got.difference(&frozen()).collect::<Vec<_>>(),
        frozen().difference(&got).collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------------------------
// Bundle: dependency graph and built artefact
// ---------------------------------------------------------------------------------------------

/// Packages the bundle links: the normal-dependency closure of `vowl-wasm`
/// with `--features ngg1,interaction` on `wasm32-unknown-unknown`.
fn bundle_packages() -> BTreeMap<String, (String, Value)> {
    let meta = metadata(&[
        "--features",
        "ngg1,interaction",
        "--filter-platform",
        "wasm32-unknown-unknown",
    ]);
    let packages: BTreeMap<String, &Value> = meta["packages"]
        .as_array()
        .expect("packages")
        .iter()
        .map(|p| (p["id"].as_str().unwrap().to_string(), p))
        .collect();
    let nodes: BTreeMap<String, &Value> = meta["resolve"]["nodes"]
        .as_array()
        .expect("resolve nodes")
        .iter()
        .map(|n| (n["id"].as_str().unwrap().to_string(), n))
        .collect();
    let root_id = meta["resolve"]["root"]
        .as_str()
        .expect("resolve root")
        .to_string();
    let mut seen = BTreeSet::new();
    let mut stack = vec![root_id];
    let mut out = BTreeMap::new();
    while let Some(id) = stack.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        let p = packages[&id];
        out.insert(
            p["name"].as_str().unwrap().to_string(),
            (
                p["version"].as_str().unwrap().to_string(),
                p["source"].clone(),
            ),
        );
        for d in nodes[&id]["deps"].as_array().expect("deps") {
            let normal = d["dep_kinds"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|k| k["kind"].is_null());
            if normal {
                stack.push(d["pkg"].as_str().unwrap().to_string());
            }
        }
    }
    out
}

#[test]
fn bundle_graph_has_ontomark_ngg1_and_no_reasoner_rdf_yaml_or_regex_crates() {
    const FORBIDDEN: &[&str] = &[
        "regex",
        "regex-automata",
        "regex-syntax",
        "aho-corasick",
        "serde_yaml",
        "serde_yml",
        "yaml-rust",
        "whelk-el",
        "ontomark-compile",
        "ontomark-reason",
        "ontomark-parse",
        "ontomark-model",
        "oxrdf",
        "oxttl",
        "oxigraph",
        "sophia_api",
        "rio_turtle",
        "rio_api",
    ];
    let graph = bundle_packages();
    let ngg = graph
        .get("ontomark-ngg1")
        .expect("VW-01: ontomark-ngg1 is not in the bundle's dependency graph for --features ngg1,interaction");
    assert!(
        crates_io(&ngg.1),
        "VW-01: the ontomark-ngg1 in the bundle graph must be the crates.io release, not {:?}",
        ngg.1
    );
    let bad: Vec<&&str> = FORBIDDEN
        .iter()
        .filter(|f| graph.contains_key(**f))
        .collect();
    assert!(
        bad.is_empty(),
        "bundle budget ({MAIN_BUNDLE_BYTES} B on main): the normal dependency graph of \
         `--features ngg1,interaction` pulled in {bad:?}; depend on `ontomark-ngg1` only"
    );
}

fn bundle_gate_required() -> bool {
    std::env::var("VOWL_BUNDLE_GATE").is_ok_and(|v| v == "1")
}

#[test]
fn built_bundle_is_a_real_wasm_module_within_budget_with_the_frozen_api() {
    let wasm = root().join("pkg/vowl_wasm_bg.wasm");
    let dts = root().join("pkg/vowl_wasm.d.ts");
    if !wasm.exists() && !dts.exists() {
        assert!(
            !bundle_gate_required(),
            "VOWL_BUNDLE_GATE=1 but pkg/ was not built: run \
             `wasm-pack build --release --target web --features ngg1,interaction` first"
        );
        eprintln!("bundle artefacts not built; run wasm-pack to exercise this check (CI does)");
        return;
    }
    let bytes = fs::read(&wasm).unwrap_or_else(|e| panic!("cannot read {}: {e}", wasm.display()));
    assert!(
        bytes.len() >= 8 && bytes[..8] == [0x00, b'a', b's', b'm', 0x01, 0x00, 0x00, 0x00],
        "pkg/vowl_wasm_bg.wasm is not a WebAssembly version-1 module (magic bytes)"
    );
    let n = bytes.len() as u64;
    assert!(
        n >= BUNDLE_FLOOR_BYTES,
        "pkg/vowl_wasm_bg.wasm is {n} B; a real vowl-wasm module is at least {BUNDLE_FLOOR_BYTES} B"
    );
    assert!(
        n <= BUNDLE_BUDGET_BYTES,
        "pkg/vowl_wasm_bg.wasm is {n} B, budget {BUNDLE_BUDGET_BYTES} B (main {MAIN_BUNDLE_BYTES} B)"
    );
    let text =
        fs::read_to_string(&dts).unwrap_or_else(|e| panic!("cannot read {}: {e}", dts.display()));
    let got = exports_of_dts(&text);
    assert_eq!(
        got,
        frozen(),
        "frozen JS API: pkg/vowl_wasm.d.ts differs from the recorded surface.\nadded: {:?}\nremoved: {:?}",
        got.difference(&frozen()).collect::<Vec<_>>(),
        frozen().difference(&got).collect::<Vec<_>>()
    );
}
