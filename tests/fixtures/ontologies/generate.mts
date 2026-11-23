// Regenerates the fixtures in this directory from owl2vowl documents.
//
// Usage (Node >= 22.6):
//   git -C <WasmVOWL> show 07ea50d0:modern/src/lib/owl2vowl.ts > owl2vowl.ts
//   # replace its one `import type { EdgeType, NodeType } from '../types/graph';`
//   # with local type aliases, then:
//   node --experimental-strip-types generate.mts <WasmVOWL>/legacy/src/app/data .
//
// Each fixture is the `engine` payload of `normaliseOwl2Vowl`, trimmed to the
// fields the WebVowl engine reads.
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { normaliseOwl2Vowl } from './owl2vowl.ts';

const [src, out] = process.argv.slice(2);
mkdirSync(out, { recursive: true });
for (const name of ['benchmark', 'foaf', 'goodrelations', 'muto', 'ontovibe', 'personasonto', 'sioc']) {
  const doc = JSON.parse(readFileSync(`${src}/${name}.json`, 'utf8'));
  const n = normaliseOwl2Vowl(doc);
  const e = n.engine;
  const fixture = {
    header: { iri: e.header.iri },
    class: e.class.map((c) => ({ id: c.id, type: c.type, label: c.label })),
    property: e.property.map((p) => ({ id: p.id, type: p.type, label: p.label, domain: p.domain, range: p.range }))
  };
  writeFileSync(`${out}/${name}.json`, JSON.stringify(fixture, null, 1) + '\n');
  console.log(name, fixture.class.length, fixture.property.length, 'dropped', n.report.dropped.length);
}
