# Ontology layout fixtures

Seven real ontologies in the merged form the `WebVowl` engine parses
(`{header, class: [{id, type, label}], property: [{id, type, label, domain,
range}]}`), used by `tests/webvowl_layout_test.rs` to check that the force
layout converges.

## Provenance

The sources are the owl2vowl JSON documents shipped with WebVOWL, as vendored
in [DreamLab-AI/WasmVOWL](https://github.com/DreamLab-AI/WasmVOWL) at
`legacy/src/app/data/` (commit `9fadfe40`). They were merged with the
normaliser `modern/src/lib/owl2vowl.ts` from WasmVOWL commit `07ea50d0`
(branch `fix/engine-contract-and-layout`) by `generate.mts` in this directory:
the base and attribute records are joined, missing endpoints are attached to a
synthesised `owl:Thing`, each undeclared datatype range gets its own node, and
the result is trimmed to the fields the engine reads. No record was dropped.

| Fixture | Nodes | Edges | Ontology |
|---|---:|---:|---|
| `benchmark.json` | 52 | 86 | WebVOWL benchmark graph |
| `foaf.json` | 56 | 74 | Friend of a Friend (FOAF), `http://xmlns.com/foaf/0.1/` |
| `goodrelations.json` | 147 | 461 | GoodRelations v1, `http://purl.org/goodrelations/v1` |
| `muto.json` | 16 | 26 | Modular Unified Tagging Ontology (MUTO), `http://purl.org/muto/core` |
| `ontovibe.json` | 55 | 70 | Ontology Visualization Benchmark (OntoViBe) 2.2 |
| `personasonto.json` | 100 | 247 | PersonasOnto 1.5 |
| `sioc.json` | 49 | 125 | SIOC Core Ontology, `http://rdfs.org/sioc/ns#` |

## Licences

These files are derived from third-party ontologies and are **not** covered by
the crate's MIT licence; each keeps the licence of its source. Every licence
below permits redistribution of a modified copy with attribution.

| Fixture | Licence | Source of the statement |
|---|---|---|
| `benchmark.json` | MIT, © 2014-2019 Vincent Link, Steffen Lohmann, Eduard Marbach, Stefan Negru, Vitalis Wiens | WebVOWL `license.txt` (the graph is WebVOWL's own test data) |
| `foaf.json` | [CC BY 1.0](http://creativecommons.org/licenses/by/1.0/), © 2000-2014 Dan Brickley and Libby Miller | [FOAF Vocabulary Specification](http://xmlns.com/foaf/spec/): applies to the specification "and accompanying documentation in RDF" |
| `goodrelations.json` | [CC BY 3.0](http://creativecommons.org/licenses/by/3.0/), Martin Hepp | `dcterms:license` in the ontology header |
| `muto.json` | [CC BY 3.0](http://creativecommons.org/licenses/by/3.0/), Steffen Lohmann | licence annotation in the ontology header |
| `ontovibe.json` | [CC BY 4.0](http://creativecommons.org/licenses/by/4.0/), Florian Haag, Steffen Lohmann | `dc:rights` in the ontology header |
| `personasonto.json` | [CC BY-SA 3.0](http://creativecommons.org/licenses/by-sa/3.0/), Stefan Negru | `dc:rights` in the ontology header. Share-alike: this derived file is distributed under CC BY-SA 3.0 too |
| `sioc.json` | Ontology terms: no licence asserted; specification text: [CC BY 1.0](http://creativecommons.org/licenses/by/1.0/), DERI / Data Science Institute, NUI Galway | [SIOC Core Ontology Specification](http://rdfs.org/sioc/spec/): the CC BY 1.0 copyright "does not apply to SIOC data formats, ontology terms, or technology". The ontology document itself carries no licence statement |

The fixtures keep only identifiers, types, labels and the domain/range
structure: comments, descriptions and other annotation text are dropped.
