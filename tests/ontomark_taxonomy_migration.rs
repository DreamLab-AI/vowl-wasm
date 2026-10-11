//! The renderer accepts taxonomy metadata from callers. It must not restore
//! the retired six-domain registry or its Markdown-only `OntologyBlock` model.
//! NGG1 numeric domain/category identifiers remain owned by ontomark-ngg1.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use syn::visit::Visit;

const LEGACY_SLUGS: &[&str] = &["ai", "aigo", "mv", "tc", "rb", "dt", "bc"];
const LEGACY_VARIANTS: &[&str] = &[
    "AI",
    "Metaverse",
    "Telecollaboration",
    "Robotics",
    "DisruptiveTech",
    "Blockchain",
];

#[derive(Default)]
struct TaxonomyFacts {
    retired_types: BTreeSet<String>,
    strings: BTreeSet<String>,
    enum_variants: Vec<BTreeSet<String>>,
}

impl<'ast> Visit<'ast> for TaxonomyFacts {
    // Doc comments are metadata, not executable registry declarations.
    fn visit_attribute(&mut self, attr: &'ast syn::Attribute) {
        if !attr.path().is_ident("doc") {
            syn::visit::visit_attribute(self, attr);
        }
    }

    fn visit_ident(&mut self, ident: &'ast syn::Ident) {
        if ident == "Domain" || ident == "OntologyBlock" {
            self.retired_types.insert(ident.to_string());
        }
    }

    fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
        self.enum_variants
            .push(item.variants.iter().map(|v| v.ident.to_string()).collect());
        syn::visit::visit_item_enum(self, item);
    }

    fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
        self.strings.insert(literal.value());
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        fn collect(
            tokens: proc_macro2::TokenStream,
            facts: &mut TaxonomyFacts,
            names: &mut BTreeSet<String>,
        ) {
            for token in tokens {
                match token {
                    proc_macro2::TokenTree::Ident(ident) => {
                        facts.visit_ident(&ident);
                        names.insert(ident.to_string());
                    }
                    proc_macro2::TokenTree::Literal(literal) => {
                        if let Ok(syn::Lit::Str(value)) = syn::parse_str(&literal.to_string()) {
                            facts.strings.insert(value.value());
                        }
                    }
                    proc_macro2::TokenTree::Group(group) => collect(group.stream(), facts, names),
                    _ => {}
                }
            }
        }
        let mut names = BTreeSet::new();
        collect(mac.tokens.clone(), self, &mut names);
        self.enum_variants.push(names);
        syn::visit::visit_macro(self, mac);
    }
}

fn violations(source: &str) -> Vec<String> {
    let file = syn::parse_file(source).expect("Rust source must parse");
    let mut facts = TaxonomyFacts::default();
    facts.visit_file(&file);
    let mut found = Vec::new();
    for name in facts.retired_types {
        found.push(format!("retired type `{name}`"));
    }
    if facts
        .strings
        .iter()
        .any(|s| s.contains("narrativegoldmine.com"))
    {
        found.push("hard-coded estate namespace".to_string());
    }
    if LEGACY_SLUGS
        .iter()
        .filter(|s| facts.strings.contains(**s))
        .count()
        >= 3
    {
        found.push("hard-coded estate slug table".to_string());
    }
    if facts.enum_variants.iter().any(|variants| {
        LEGACY_VARIANTS
            .iter()
            .filter(|v| variants.contains(**v))
            .count()
            >= 3
    }) {
        found.push("renamed estate domain enum".to_string());
    }
    found
}

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    if !dir.exists() {
        return;
    }
    for entry in fs::read_dir(dir).expect("source directory") {
        let path = entry.expect("source entry").path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn vw03_no_local_taxonomy_registry_or_retired_markdown_types_remain() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut sources = Vec::new();
    for dir in ["src", "examples", "benches"] {
        rust_sources(&root.join(dir), &mut sources);
    }
    if root.join("build.rs").exists() {
        sources.push(root.join("build.rs"));
    }
    assert!(sources.iter().any(|p| p.ends_with("src/lib.rs")));
    let found: Vec<_> = sources
        .iter()
        .flat_map(|path| {
            violations(&fs::read_to_string(path).expect("source file"))
                .into_iter()
                .map(|v| format!("{}: {v}", path.strip_prefix(&root).unwrap().display()))
        })
        .collect();
    assert!(found.is_empty(), "VW-03: {}", found.join("\n"));
}

#[test]
fn taxonomy_guard_rejects_renamed_moved_and_hidden_legacy_definitions() {
    for source in [
        "pub enum Domain { Custom }",
        "type Domain = u8;",
        "pub struct OntologyBlock { pub id: u32 }",
        "mod nested { enum Renamed { AI, Metaverse, Robotics } }",
        "const TABLE: &[&str] = &[\"ai\", \"mv\", \"tc\"];",
        "#[cfg(any())] const TABLE: &[&str] = &[\"ai\", \"mv\", \"tc\"];",
        "fn renamed() -> &'static str { \"http://narrativegoldmine.com/ai#\" }",
        "macro_rules! restore { () => { enum Domain { Custom } } }",
        "macro_rules! restore { () => { enum Renamed { AI, Metaverse, Robotics } } }",
        "const TABLE: &[&str] = registry![\"ai\", \"mv\", \"tc\"];",
    ] {
        assert!(!violations(source).is_empty(), "guard missed: {source}");
    }
    assert!(violations(
        "// enum Domain { AI, Metaverse, Robotics }\n\
         /// http://narrativegoldmine.com/ai#\n\
         pub struct CallerMetadata { pub domain: Option<String> }"
    )
    .is_empty());
}
