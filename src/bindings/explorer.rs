//! `NggExplorer` — the WASM surface for the NarrativeGoldmine explorer worker
//! (ADR-NG-001 §3). One instance per worker: load a tier with `load_csr`, expose
//! the live position buffer to JS zero-copy via `positions_ptr`/`positions_len`
//! (or copy it out with `write_positions_into` on the transferable path), and
//! drive the sim with `tick`/`is_finished`/`get_alpha`/`set_*`, pinning dragged
//! nodes with `pin_node`/`unpin_node`.
//!
//! This replaces the old `get_graph_data()` boundary-serialisation hot path
//! (deleted from default builds, kept behind the `debug-serde` feature on the
//! legacy `WebVowl` struct). Nothing here allocates across the boundary per
//! frame; only the position pointer crosses.

use crate::layout::csr_sim::CsrSimulation;
use wasm_bindgen::prelude::*;

/// Check `loadArrays` input so that [`CsrSimulation::load_arrays`] cannot panic.
fn validate_arrays(
    positions: &[f32],
    edge_a: &[u32],
    edge_b: &[u32],
    edge_type: &[u8],
) -> Result<(), String> {
    if positions.len() % 2 != 0 {
        return Err(format!(
            "loadArrays: positions must be interleaved [x,y,…]; got an odd length of {}",
            positions.len()
        ));
    }
    if let Some(i) = positions.iter().position(|v| !v.is_finite()) {
        return Err(format!(
            "loadArrays: positions[{i}] is not finite ({})",
            positions[i]
        ));
    }
    if edge_a.len() != edge_b.len() || edge_b.len() != edge_type.len() {
        return Err(format!(
            "loadArrays: edge arrays must be parallel; got {}/{}/{}",
            edge_a.len(),
            edge_b.len(),
            edge_type.len()
        ));
    }
    let n = positions.len() / 2;
    for (e, (&a, &b)) in edge_a.iter().zip(edge_b).enumerate() {
        if a as usize >= n || b as usize >= n {
            return Err(format!(
                "loadArrays: edge {e} ({a}, {b}) names a node outside 0..{n}"
            ));
        }
    }
    Ok(())
}

/// The explorer's physics engine, as seen from the worker in JS.
#[wasm_bindgen]
pub struct NggExplorer {
    sim: CsrSimulation,
}

impl Default for NggExplorer {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl NggExplorer {
    /// Construct an empty explorer. Call `load_csr` before ticking.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            sim: CsrSimulation::new(),
        }
    }

    /// Load an NGG1 tier: parse the bytes, seed positions from the node
    /// warm-start, build springs from the CSR adjacency, reset the temperature.
    /// Swaps the whole scope (DDD INV-2). Errors on a malformed/truncated buffer.
    #[wasm_bindgen(js_name = loadCsr)]
    pub fn load_csr(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
        self.sim
            .load_csr(bytes)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Load a plain graph without encoding NGG1 first: interleaved warm-start
    /// `positions` `[x0, y0, x1, y1, …]` plus three parallel edge arrays, where
    /// `edgeA[e]`/`edgeB[e]` are node indices and `edgeType[e]` classifies the
    /// spring. Swaps the whole scope, clears every pin and resets the
    /// temperature, exactly as `loadCsr` does.
    ///
    /// **Index order.** Node `i` is `positions[2i], positions[2i + 1]`, and edge
    /// endpoints refer to that index. The order is stable until the next load,
    /// so the host can keep its own per-node metadata in the same order and read
    /// positions back by index.
    ///
    /// **What may be passed.** Self-loops (`a == b`) and duplicate edges are
    /// accepted. `tick` skips self-loops, so they exert no force; a duplicate
    /// edge applies its spring once per copy, pulling twice as hard. Edges are
    /// treated as undirected springs. An empty graph is valid and reports
    /// finished.
    ///
    /// # Errors
    ///
    /// Rejects, leaving the previously loaded graph untouched, an odd
    /// `positions` length, a non-finite position, edge arrays of differing
    /// lengths, and any endpoint index at or beyond the node count. It never
    /// panics, since a panic would abort the WASM instance.
    #[wasm_bindgen(js_name = loadArrays)]
    pub fn load_arrays(
        &mut self,
        positions: &[f32],
        edge_a: &[u32],
        edge_b: &[u32],
        edge_type: &[u8],
    ) -> Result<(), JsValue> {
        validate_arrays(positions, edge_a, edge_b, edge_type).map_err(|e| JsValue::from_str(&e))?;
        self.sim.load_arrays(
            positions.to_vec(),
            edge_a.to_vec(),
            edge_b.to_vec(),
            edge_type.to_vec(),
        );
        Ok(())
    }

    /// Advance one simulation step. No-op once settled.
    #[wasm_bindgen(js_name = tick)]
    pub fn tick(&mut self) {
        self.sim.tick();
    }

    /// `true` once the annealing temperature reached the floor.
    #[wasm_bindgen(js_name = isFinished)]
    pub fn is_finished(&self) -> bool {
        self.sim.is_finished()
    }

    /// Current alpha (annealing temperature) for HUD/telemetry.
    #[wasm_bindgen(js_name = getAlpha)]
    pub fn get_alpha(&self) -> f32 {
        self.sim.alpha()
    }

    /// On-screen node count of the loaded scope.
    #[wasm_bindgen(js_name = nodeCount)]
    pub fn node_count(&self) -> usize {
        self.sim.node_count()
    }

    /// Directed edge count of the loaded scope.
    #[wasm_bindgen(js_name = edgeCount)]
    pub fn edge_count(&self) -> usize {
        self.sim.edge_count()
    }

    /// Pointer to the live interleaved position buffer. JS builds a
    /// `new Float32Array(wasm.memory.buffer, positionsPtr(), positionsLen())`
    /// over WASM linear memory — zero copy (VisionClaw `get_*_ptr` idiom). The
    /// view is invalidated by the next `loadCsr` (possible reallocation) and by
    /// any WASM memory growth; re-read the pointer after either.
    #[wasm_bindgen(js_name = positionsPtr)]
    pub fn positions_ptr(&self) -> *const f32 {
        self.sim.positions_ptr()
    }

    /// Length of the position buffer in `f32` elements (`nodeCount * 2`).
    #[wasm_bindgen(js_name = positionsLen)]
    pub fn positions_len(&self) -> usize {
        self.sim.positions_len()
    }

    /// Copy live positions into the caller's buffer (the transferable ping-pong
    /// path used when `SharedArrayBuffer` is unavailable). Copies the overlap.
    #[wasm_bindgen(js_name = writePositionsInto)]
    pub fn write_positions_into(&self, out: &mut [f32]) {
        self.sim.write_positions_into(out);
    }

    /// Live-tune a scalar parameter by `protocol.ts` key: `charge`,
    /// `linkDistance`, `linkStrength`, `damping`, `centerGravity`, `alphaMin`,
    /// `reheat`. Unknown keys are ignored.
    #[wasm_bindgen(js_name = setParam)]
    pub fn set_param(&mut self, key: &str, val: f32) {
        self.sim.set_param(key, val);
    }

    /// Set the Barnes-Hut opening angle for many-body repulsion. The default is
    /// 0.9 (d3-force); larger is faster but coarser, and `0` (or below) selects
    /// the exact O(n²) all-pairs loop.
    #[wasm_bindgen(js_name = setTheta)]
    pub fn set_theta(&mut self, theta: f32) {
        self.sim.set_theta(theta);
    }

    /// Hold node `index` at `(x, y)` while the host drags it; its neighbours
    /// keep reacting to it. Returns `false`, changing nothing, for an index
    /// outside the loaded scope or a non-finite coordinate, so a stale UI event
    /// cannot take the worker down. Pair with `reheat` to wake a settled scope.
    #[wasm_bindgen(js_name = pinNode)]
    pub fn pin_node(&mut self, index: u32, x: f32, y: f32) -> bool {
        self.sim.pin(index as usize, x, y)
    }

    /// Release a pinned node back to the layout. Returns `false` for an index
    /// outside the loaded scope. `loadCsr` clears every pin.
    #[wasm_bindgen(js_name = unpinNode)]
    pub fn unpin_node(&mut self, index: u32) -> bool {
        self.sim.unpin(index as usize)
    }

    /// Re-heat and resume a settled scope (convenience for `setParam("reheat")`).
    #[wasm_bindgen(js_name = reheat)]
    pub fn reheat(&mut self, alpha: f32) {
        self.sim.reheat(alpha);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Native tests cannot build a `JsValue`, so they drive the same
    /// validate-then-load path through the sim directly.
    fn load(
        sim: &mut CsrSimulation,
        p: &[f32],
        a: &[u32],
        b: &[u32],
        t: &[u8],
    ) -> Result<(), String> {
        validate_arrays(p, a, b, t)?;
        sim.load_arrays(p.to_vec(), a.to_vec(), b.to_vec(), t.to_vec());
        Ok(())
    }

    fn seeded() -> CsrSimulation {
        let mut sim = CsrSimulation::new();
        load(
            &mut sim,
            &[0.0, 0.0, 10.0, 0.0, 0.0, 10.0],
            &[0, 1],
            &[1, 2],
            &[0, 1],
        )
        .unwrap();
        sim
    }

    #[test]
    fn valid_graph_loads_and_settles_finite() {
        let mut sim = seeded();
        assert_eq!((sim.node_count(), sim.edge_count()), (3, 2));
        for _ in 0..5000 {
            sim.tick();
        }
        assert!(sim.is_finished());
        let mut out = vec![0.0; 6];
        sim.write_positions_into(&mut out);
        assert!(out.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn self_loops_and_duplicates_are_accepted() {
        let mut sim = CsrSimulation::new();
        load(
            &mut sim,
            &[0.0, 0.0, 5.0, 0.0],
            &[0, 0, 0],
            &[0, 1, 1],
            &[0, 0, 0],
        )
        .unwrap();
        assert_eq!(sim.edge_count(), 3);
        for _ in 0..50 {
            sim.tick();
        }
        let mut out = vec![0.0; 4];
        sim.write_positions_into(&mut out);
        assert!(out.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn invalid_inputs_error_and_keep_the_previous_graph() {
        let mut sim = seeded();
        type Case<'a> = (&'a [f32], &'a [u32], &'a [u32], &'a [u8], &'a str);
        let cases: [Case; 7] = [
            (&[0.0, 0.0, 1.0], &[], &[], &[], "odd length"),
            (&[0.0, f32::NAN], &[], &[], &[], "not finite"),
            (&[f32::INFINITY, 0.0], &[], &[], &[], "not finite"),
            (&[0.0, 0.0, 1.0, 1.0], &[0], &[1, 0], &[0], "parallel"),
            (&[0.0, 0.0, 1.0, 1.0], &[0], &[1], &[], "parallel"),
            (&[0.0, 0.0, 1.0, 1.0], &[0], &[2], &[0], "outside 0..2"),
            (
                &[0.0, 0.0, 1.0, 1.0],
                &[u32::MAX],
                &[0],
                &[0],
                "outside 0..2",
            ),
        ];
        for (p, a, b, t, want) in cases {
            let err = load(&mut sim, p, a, b, t).unwrap_err();
            assert!(err.contains(want), "{err} should contain {want}");
            assert_eq!((sim.node_count(), sim.edge_count()), (3, 2));
        }
    }

    #[test]
    fn empty_graph_is_accepted_and_finished() {
        let mut sim = seeded();
        load(&mut sim, &[], &[], &[], &[]).unwrap();
        assert_eq!((sim.node_count(), sim.edge_count()), (0, 0));
        assert!(sim.is_finished());
    }
}
