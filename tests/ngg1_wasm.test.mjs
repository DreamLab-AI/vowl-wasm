// Run after wasm-pack build: node --test tests/ngg1_wasm.test.mjs
// The fixture is byte-identical to ontomark-ngg1::worked_example (native gate).
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import {
  initSync, decodeNgg1, encodeNgg1, ngg1Constants, NggExplorer, WebVowl, version,
} from '../pkg/vowl_wasm.js';

initSync({ module: readFileSync(new URL('../pkg/vowl_wasm_bg.wasm', import.meta.url)) });
const golden = new Uint8Array(readFileSync(new URL('./fixtures/ngg1-3n2e.bin', import.meta.url)));

function fields(graph) {
  return [
    graph.positions(), graph.ids(), graph.domains(), graph.categories(), graph.flags(),
    graph.degrees(), graph.labels(), graph.iris(), graph.rowPtr(), graph.colIdx(), graph.edgeType(),
  ];
}

test('the golden decodes all fields and re-encodes byte for byte through WASM', () => {
  const graph = decodeNgg1(golden);
  try {
    assert.equal(version(), '0.2.0');
    assert.equal(graph.nodeCount(), 3);
    assert.equal(graph.edgeCount(), 2);
    const data = fields(graph);
    const types = [Float32Array, Uint32Array, Uint16Array, Uint16Array, Uint8Array, Uint32Array,
      Array, Array, Uint32Array, Uint32Array, Uint8Array];
    data.forEach((value, i) => assert(value instanceof types[i]));
    assert.deepEqual(data.map((v) => Array.from(v)), [
      [0, 0, 1, 0, 0, 1], [0, 1, 2], [0, 0, 0], [0, 1, 1], [1, 0, 0], [1, 2, 1],
      ['AI', 'ML', 'NLP'], ['ng:0', 'ng:1', 'ng:2'], [0, 0, 2, 2], [0, 2], [0, 1],
    ]);
    assert.deepEqual(encodeNgg1(...data), golden);
    data[1][0] = 99;
    data[6][0] = 'changed';
    assert.equal(graph.ids()[0], 0);
    assert.equal(graph.labels()[0], 'AI');
  } finally {
    graph.free();
  }
});

test('custom taxonomy, sentinels, Unicode and unknown codes survive synthetic graph encoding', () => {
  const constants = ngg1Constants();
  assert.equal(Object.getPrototypeOf(constants), Object.prototype);
  assert.equal(constants.NGG1_NODE_STRIDE, 24);
  assert.equal(constants.DOMAIN_NONE, 65535);
  assert.equal(constants.CATEGORY_NONE, 65535);
  assert.equal(constants.FLAG_META, 64);
  const graph = decodeNgg1(golden);
  const data = fields(graph);
  graph.free();
  data[0] = new Float32Array([3.25, -7.5, 8, 9, 10, 11]);
  data[1] = new Uint32Array([4001, 123, 0xffffffff]);
  data[2] = new Uint16Array([12, 99, constants.DOMAIN_NONE]);
  data[3] = new Uint16Array([45, constants.CATEGORY_NONE, 2345]);
  data[4] = new Uint8Array([128, constants.FLAG_META, 0]);
  data[6] = ['Café', '星雲', ''];
  data[7] = ['urn:custom:α', 'https://example.org/星', ''];
  data[10] = new Uint8Array([constants.EDGE_SUBCLASS, 99]);
  const encoded = encodeNgg1(...data);
  const decoded = decodeNgg1(encoded);
  try {
    assert.deepEqual(fields(decoded), data);
    assert.deepEqual(encodeNgg1(...fields(decoded)), encoded);
  } finally {
    decoded.free();
  }
});

test('empty tiers roundtrip and encoded graphs run through the existing zero-copy explorer', () => {
  const empty = encodeNgg1(new Float32Array(), new Uint32Array(), new Uint16Array(),
    new Uint16Array(), new Uint8Array(), new Uint32Array(), [], [],
    new Uint32Array([0]), new Uint32Array(), new Uint8Array());
  const graph = decodeNgg1(empty);
  assert.equal(graph.nodeCount(), 0);
  assert.equal(graph.edgeCount(), 0);
  graph.free();
  const explorer = new NggExplorer();
  try {
    explorer.loadCsr(golden);
    for (let i = 0; i < 300; i++) explorer.tick();
    const positions = new Float32Array(explorer.positionsLen());
    explorer.writePositionsInto(positions);
    assert.equal(positions.length, 6);
    assert(positions.every(Number.isFinite));
    assert.throws(() => explorer.loadCsr(new Uint8Array()), /NGG1:/);
    assert.equal(explorer.nodeCount(), 3);
    explorer.tick();
  } finally {
    explorer.free();
  }
  const legacy = new WebVowl();
  assert.equal(typeof legacy.getMetadata, 'function');
  assert.equal(typeof legacy.getNodePositions, 'function');
  legacy.free();
});

test('decoder rejects every truncation, malformed CSR, version and UTF-8', () => {
  for (let n = 0; n < golden.length; n++) {
    assert.throws(() => decodeNgg1(golden.subarray(0, n)), /NGG1:/);
  }
  for (const mutate of [
    (b) => { b[0] = 0; },
    (b) => { b[4] = 99; },
    (b) => { new DataView(b.buffer).setUint32(120, 99, true); },
    (b) => { new DataView(b.buffer).setUint32(104, 1, true); },
    (b) => { b[b.length - 1] = 255; },
  ]) {
    const bytes = golden.slice();
    mutate(bytes);
    assert.throws(() => decodeNgg1(bytes), /NGG1:/);
  }
});

test('encoder rejects every mismatched field length and invalid CSR', () => {
  const graph = decodeNgg1(golden);
  const data = fields(graph);
  graph.free();
  for (let i = 0; i < data.length; i++) {
    const bad = data.slice();
    bad[i] = data[i].slice(0, -1);
    assert.throws(() => encodeNgg1(...bad), /NGG1:/, `field ${i}`);
  }
  for (const [slot, value] of [
    [8, new Uint32Array([1, 1, 2, 2])],
    [8, new Uint32Array([0, 2, 1, 2])],
    [8, new Uint32Array([0, 0, 1, 1])],
    [9, new Uint32Array([99, 2])],
  ]) {
    const bad = data.slice();
    bad[slot] = value;
    assert.throws(() => encodeNgg1(...bad), /NGG1:/);
  }
});
