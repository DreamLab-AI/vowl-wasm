//! WASM bindings for JavaScript interop

#[cfg(feature = "ngg1")]
#[cfg_attr(docsrs, doc(cfg(feature = "ngg1")))]
pub mod explorer;

#[cfg(feature = "ngg1")]
#[cfg_attr(docsrs, doc(cfg(feature = "ngg1")))]
pub mod ngg;

use crate::{
    graph::{builder::GraphBuilder, pinning::PinManager, GraphStatistics, VowlGraph},
    layout::{simulation::ForceSimulation, LayoutAlgorithm},
    ontology::{parser::StandardParser, OntologyMetadata, OntologyParser},
};
#[cfg(feature = "debug-serde")]
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

#[cfg(feature = "interaction")]
use crate::interaction::{find_closest_node_hit, NodeHitTest, Ray};
#[cfg(feature = "interaction")]
use nalgebra::Vector3;

/// Main WebVOWL WASM interface
#[wasm_bindgen]
pub struct WebVowl {
    graph: Option<VowlGraph>,
    simulation: ForceSimulation,
    pin_manager: PinManager,
    /// Header of the most recently loaded ontology, surfaced by `getMetadata`.
    metadata: Option<OntologyMetadata>,
}

impl Default for WebVowl {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl WebVowl {
    /// Create a new WebVOWL instance
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            graph: None,
            simulation: ForceSimulation::new(),
            pin_manager: PinManager::new(),
            metadata: None,
        }
    }

    /// Load ontology from JSON string
    ///
    /// Replaces the current graph, clears pins and restarts the simulation
    /// (alpha back to its starting value). Simulation settings made earlier
    /// with [`set_link_distance`](Self::set_link_distance),
    /// [`set_charge_strength`](Self::set_charge_strength) and
    /// [`set_center`](Self::set_center) are kept.
    #[wasm_bindgen(js_name = loadOntology)]
    pub fn load_ontology(&mut self, json: &str) -> std::result::Result<(), JsValue> {
        let parser = StandardParser::new();
        let ontology_data = parser
            .parse(json)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        parser
            .validate(&ontology_data)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        let graph = GraphBuilder::from_ontology(&ontology_data)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        self.metadata = Some(ontology_data.metadata.clone());
        self.pin_manager = PinManager::new();
        // Per-node radii describe the previous graph's nodes; the uniform
        // collision radius and every other setting carry over.
        self.simulation.clear_node_radii();
        // Restart the simulation for the new graph but keep its settings:
        // `setLinkDistance` / `setChargeStrength` / `setCenter` made before
        // this call still apply.
        self.simulation.reset();
        self.graph = Some(graph);
        Ok(())
    }

    /// Initialize the force simulation
    #[wasm_bindgen(js_name = initSimulation)]
    pub fn init_simulation(&mut self) -> std::result::Result<(), JsValue> {
        let graph = self
            .graph
            .as_mut()
            .ok_or_else(|| JsValue::from_str("No graph loaded"))?;

        self.simulation
            .initialize(graph)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        Ok(())
    }

    /// Run simulation for n iterations
    #[wasm_bindgen(js_name = runSimulation)]
    pub fn run_simulation(&mut self, iterations: usize) -> std::result::Result<(), JsValue> {
        let graph = self
            .graph
            .as_mut()
            .ok_or_else(|| JsValue::from_str("No graph loaded"))?;

        self.simulation
            .run(graph, iterations)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        Ok(())
    }

    /// Perform one simulation tick
    #[wasm_bindgen(js_name = tick)]
    pub fn tick(&mut self) -> std::result::Result<(), JsValue> {
        let graph = self
            .graph
            .as_mut()
            .ok_or_else(|| JsValue::from_str("No graph loaded"))?;

        self.simulation
            .tick(graph)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        Ok(())
    }

    /// Check if simulation is finished
    #[wasm_bindgen(js_name = isFinished)]
    pub fn is_finished(&self) -> bool {
        self.simulation.is_finished()
    }

    /// Get current alpha value
    #[wasm_bindgen(js_name = getAlpha)]
    pub fn get_alpha(&self) -> f64 {
        self.simulation.alpha()
    }

    /// Re-heat a settled simulation so `tick` moves nodes again, for example
    /// after a filter or a drag. Mirrors `NggExplorer.reheat`.
    #[wasm_bindgen(js_name = reheat)]
    pub fn reheat(&mut self, alpha: f64) {
        self.simulation.reheat(alpha);
    }

    /// Set simulation center
    #[wasm_bindgen(js_name = setCenter)]
    pub fn set_center(&mut self, x: f64, y: f64) {
        self.simulation.set_center(x, y);
    }

    /// Set link distance
    #[wasm_bindgen(js_name = setLinkDistance)]
    pub fn set_link_distance(&mut self, distance: f64) {
        self.simulation.set_link_distance(distance);
    }

    /// Set charge strength
    #[wasm_bindgen(js_name = setChargeStrength)]
    pub fn set_charge_strength(&mut self, strength: f64) {
        self.simulation.set_charge_strength(strength);
    }

    /// Give every node a collision radius, in layout units (d3-force
    /// `forceCollide`). `0`, the default, turns collision off; a negative or
    /// non-finite value is ignored. Kept across `loadOntology`.
    ///
    /// With collision on, no two nodes settle closer than the sum of their
    /// radii (less a few percent where other forces push back). Per-node radii
    /// from [`set_node_radii`](Self::set_node_radii) take precedence.
    #[wasm_bindgen(js_name = setCollisionRadius)]
    pub fn set_collision_radius(&mut self, radius: f64) {
        self.simulation.set_collision_radius(radius);
    }

    /// Give each node its own collision radius, in node index order (the order
    /// of [`get_node_ids`](Self::get_node_ids)), for example from its drawn
    /// size or type. An empty array clears them. Cleared by `loadOntology`.
    ///
    /// Throws when no graph is loaded, when the length is not the node count,
    /// or when a radius is negative or not finite; the previous radii are then
    /// kept.
    #[wasm_bindgen(js_name = setNodeRadii)]
    pub fn set_node_radii(&mut self, radii: &[f64]) -> std::result::Result<(), JsValue> {
        let Some(graph) = self.graph.as_ref() else {
            return Err(JsValue::from_str("No graph loaded"));
        };
        self.simulation
            .set_node_radii(radii, graph.node_count())
            .map_err(|e| JsValue::from_str(&e))
    }

    /// Fraction of an overlap removed per collision pass, clamped to `[0, 1]`
    /// (d3-force `forceCollide.strength`; default `1`).
    #[wasm_bindgen(js_name = setCollisionStrength)]
    pub fn set_collision_strength(&mut self, strength: f64) {
        self.simulation.set_collision_strength(strength);
    }

    /// Collision passes per tick (d3-force `forceCollide.iterations`; default
    /// `1`). Zero is ignored.
    #[wasm_bindgen(js_name = setCollisionIterations)]
    pub fn set_collision_iterations(&mut self, iterations: u32) {
        self.simulation
            .set_collision_iterations(iterations as usize);
    }

    /// Per-node pull towards the centre (d3-force `forceX` / `forceY`),
    /// clamped to `[0, 1]`. `0`, the default, turns it off. A weak value such
    /// as `0.05` draws disconnected components in around the main one.
    #[wasm_bindgen(js_name = setGravityStrength)]
    pub fn set_gravity_strength(&mut self, strength: f64) {
        self.simulation.set_gravity_strength(strength);
    }

    /// Fraction of alpha lost per tick, in `(0, 1)` (default `0.0228`, about
    /// 300 ticks). A smaller value anneals longer. Out-of-range values are
    /// ignored.
    #[wasm_bindgen(js_name = setAlphaDecay)]
    pub fn set_alpha_decay(&mut self, decay: f64) {
        self.simulation.set_alpha_decay(decay);
    }

    /// Alpha below which `isFinished` turns true, in `(0, 1)` (default
    /// `0.001`). Out-of-range values are ignored.
    #[wasm_bindgen(js_name = setAlphaMin)]
    pub fn set_alpha_min(&mut self, alpha_min: f64) {
        self.simulation.set_alpha_min(alpha_min);
    }

    /// Fraction of velocity **kept** per tick, in `[0, 1]` (default `0.6`,
    /// which is d3-force's `velocityDecay(0.4)`). Out-of-range values are
    /// ignored.
    #[wasm_bindgen(js_name = setVelocityDecay)]
    pub fn set_velocity_decay(&mut self, retained: f64) {
        self.simulation.set_velocity_decay(retained);
    }

    /// Filter hierarchy by depth
    #[wasm_bindgen(js_name = filterHierarchy)]
    pub fn filter_hierarchy(&mut self, depth: usize) -> std::result::Result<(), JsValue> {
        let graph = self
            .graph
            .as_mut()
            .ok_or_else(|| JsValue::from_str("No graph loaded"))?;

        graph.apply_hierarchy_folding(depth);
        Ok(())
    }

    /// Get graph data as JSON.
    ///
    /// Full-graph serde serialisation across the WASM boundary — the O(n) hot
    /// path deleted from the NarrativeGoldmine explorer (ADR-NG-001 §3). Retained
    /// only behind the `debug-serde` feature for diagnostics; the live explorer
    /// reads positions zero-copy via `NggExplorer::positions_ptr` instead.
    #[cfg(feature = "debug-serde")]
    #[wasm_bindgen(js_name = getGraphData)]
    pub fn get_graph_data(&self) -> std::result::Result<JsValue, JsValue> {
        let graph = self
            .graph
            .as_ref()
            .ok_or_else(|| JsValue::from_str("No graph loaded"))?;

        let data = GraphData::from_graph(graph);
        serde_wasm_bindgen::to_value(&data).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Get node count
    #[wasm_bindgen(js_name = getNodeCount)]
    pub fn get_node_count(&self) -> usize {
        self.graph.as_ref().map(|g| g.node_count()).unwrap_or(0)
    }

    /// Get edge count
    #[wasm_bindgen(js_name = getEdgeCount)]
    pub fn get_edge_count(&self) -> usize {
        self.graph.as_ref().map(|g| g.edge_count()).unwrap_or(0)
    }

    /// Node ids, in the same index order as [`get_node_positions`].
    ///
    /// Call this once after [`load_ontology`](Self::load_ontology) to build the
    /// index-to-id map, then read positions per frame without touching strings
    /// again. Returns an empty array when no ontology has been loaded.
    ///
    /// ```javascript
    /// const ids = vowl.getNodeIds();
    /// // ids[i] is the node whose coordinates live at getNodePositions()[2i], [2i + 1]
    /// ```
    ///
    /// [`get_node_positions`]: Self::get_node_positions
    #[wasm_bindgen(js_name = getNodeIds)]
    pub fn get_node_ids(&self) -> Vec<String> {
        self.graph
            .as_ref()
            .map(|g| g.nodes().iter().map(|n| n.id.clone()).collect())
            .unwrap_or_default()
    }

    /// Live node positions, interleaved `x, y` in node index order.
    ///
    /// Returns a freshly allocated `Float32Array` of `getNodeCount() * 2`
    /// elements: element `2i` is node `i`'s `x` and element `2i + 1` its `y`.
    /// Empty when no ontology has been loaded.
    ///
    /// # Index order
    ///
    /// Index `i` is the node's insertion position — the order
    /// [`load_ontology`](Self::load_ontology) built the graph in. That order is
    /// **stable for the lifetime of a loaded graph**: ticking, pinning,
    /// filtering and folding never reorder or remove nodes (the filters flip a
    /// visibility flag and leave the node in place), so an index captured once
    /// after `loadOntology` stays valid until the next `loadOntology`. The
    /// length always matches `getNodeCount()`, and therefore the
    /// `basic.node_count` reported by [`get_statistics`](Self::get_statistics).
    ///
    /// Pair it once with [`get_node_ids`](Self::get_node_ids) to map index to
    /// node id:
    ///
    /// ```javascript
    /// const ids = vowl.getNodeIds();       // once, after loadOntology
    ///
    /// function frame() {                   // per frame
    ///     vowl.tick();
    ///     const xy = vowl.getNodePositions();
    ///     for (let i = 0; i < ids.length; i++) {
    ///         moveNode(ids[i], xy[2 * i], xy[2 * i + 1]);
    ///     }
    /// }
    /// ```
    ///
    /// Positions are `f64` internally and are narrowed to `f32` here, which is
    /// exact enough for screen coordinates and halves the bytes crossing the
    /// boundary. This is the always-available replacement for the
    /// `debug-serde`-gated `getGraphData()` when all a caller wants is
    /// coordinates; it allocates one typed array per call instead of
    /// serialising the whole graph. To reuse one buffer across frames, call
    /// [`write_positions_into`](Self::write_positions_into) instead.
    #[wasm_bindgen(js_name = getNodePositions)]
    pub fn get_node_positions(&self) -> Vec<f32> {
        let Some(graph) = self.graph.as_ref() else {
            return Vec::new();
        };

        let nodes = graph.nodes();
        let mut out = Vec::with_capacity(nodes.len() * 2);
        for node in nodes {
            out.push(node.visual.x as f32);
            out.push(node.visual.y as f32);
        }
        out
    }

    /// Copy live positions into a caller-owned buffer, allocating nothing.
    ///
    /// Writes interleaved `x, y` in the same index order as
    /// [`get_node_positions`](Self::get_node_positions) and returns the number
    /// of `f32` elements written. It writes the overlap — at most
    /// `min(out.len(), getNodeCount() * 2)` elements — so a short buffer
    /// truncates rather than failing, and the tail of a longer one is left
    /// untouched. This mirrors `NggExplorer::write_positions_into`, the
    /// equivalent on the `ngg1` binary-tier path.
    ///
    /// ```javascript
    /// const xy = new Float32Array(vowl.getNodeCount() * 2);  // once
    ///
    /// function frame() {                                     // per frame
    ///     vowl.tick();
    ///     const written = vowl.writePositionsInto(xy);
    ///     draw(xy.subarray(0, written));
    /// }
    /// ```
    ///
    /// `wasm-bindgen` copies the typed array into WASM memory and back out
    /// again around this call, so what this saves over
    /// [`get_node_positions`](Self::get_node_positions) is the per-frame
    /// allocation, not the copy. The genuinely copy-free path is
    /// `NggExplorer::positions_ptr`, which requires the `ngg1` tier format.
    #[wasm_bindgen(js_name = writePositionsInto)]
    pub fn write_positions_into(&self, out: &mut [f32]) -> usize {
        let Some(graph) = self.graph.as_ref() else {
            return 0;
        };

        let mut written = 0;
        for node in graph.nodes() {
            let Some(slot) = out.get_mut(written..written + 2) else {
                break;
            };
            slot[0] = node.visual.x as f32;
            slot[1] = node.visual.y as f32;
            written += 2;
        }
        written
    }

    /// Ontology header of the most recently loaded ontology.
    ///
    /// Returns the OWL/RDF header fields extracted at load time — the ontology
    /// IRI, and the optional version, title and description — as a plain
    /// JavaScript object:
    ///
    /// ```javascript
    /// const meta = vowl.getMetadata();
    /// // { iri: "http://example.org/onto", version: "1.2.0",
    /// //   title: "Example", description: null }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if no ontology has been loaded yet, or if the metadata
    /// cannot be serialised across the WASM boundary.
    #[wasm_bindgen(js_name = getMetadata)]
    pub fn get_metadata(&self) -> std::result::Result<JsValue, JsValue> {
        let metadata = self
            .metadata
            .as_ref()
            .ok_or_else(|| JsValue::from_str("No ontology loaded"))?;

        serde_wasm_bindgen::to_value(metadata).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Get comprehensive graph statistics
    ///
    /// Returns detailed statistics including:
    /// - Basic metrics (node count, edge count, density)
    /// - Degree statistics (min, max, average, distribution)
    /// - Connected components analysis
    /// - OWL2 specific metrics (functional properties, equivalences, etc.)
    /// - Property distribution (by type)
    /// - Class distribution
    ///
    /// # Returns
    /// JSON object with complete graph statistics
    ///
    /// # Example (JavaScript)
    /// ```javascript
    /// const stats = webvowl.getStatistics();
    /// console.log(stats.basic.node_count);  // Total nodes
    /// console.log(stats.degree.avg_degree); // Average degree
    /// console.log(stats.owl2.functional_properties); // Functional property count
    /// ```
    #[wasm_bindgen(js_name = getStatistics)]
    pub fn get_statistics(&self) -> std::result::Result<JsValue, JsValue> {
        let graph = self
            .graph
            .as_ref()
            .ok_or_else(|| JsValue::from_str("No graph loaded"))?;

        let graph_stats = GraphStatistics::calculate(graph);
        serde_wasm_bindgen::to_value(&graph_stats).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Check which node (if any) is clicked by a ray from the camera
    ///
    /// This method performs ray-sphere intersection testing to determine
    /// which node was clicked in 3D space.
    ///
    /// # Arguments
    /// * `ray_origin` - Ray origin as [x, y, z] array
    /// * `ray_direction` - Ray direction as [x, y, z] array (will be normalized)
    ///
    /// # Returns
    /// * `Some(node_id)` - ID of the closest clicked node
    /// * `None` - No node was clicked
    ///
    /// # Example (from JavaScript)
    /// ```javascript
    /// const nodeId = webvowl.checkNodeClick(
    ///     [0, 0, 10],  // Camera position
    ///     [0, 0, -1]   // Looking down -Z axis
    /// );
    /// if (nodeId) {
    ///     console.log(`Clicked node: ${nodeId}`);
    /// }
    /// ```
    ///
    /// # Performance
    /// This method is optimized for graphs with up to 1,000 nodes.
    /// For larger graphs, it should still complete in < 1ms on modern hardware.
    #[cfg(feature = "interaction")]
    #[cfg_attr(docsrs, doc(cfg(feature = "interaction")))]
    #[wasm_bindgen(js_name = checkNodeClick)]
    pub fn check_node_click(
        &self,
        ray_origin: Vec<f32>,
        ray_direction: Vec<f32>,
    ) -> Option<String> {
        // Validate input arrays
        if ray_origin.len() != 3 || ray_direction.len() != 3 {
            web_sys::console::warn_1(&JsValue::from_str(
                "checkNodeClick: ray_origin and ray_direction must have 3 elements",
            ));
            return None;
        }

        let graph = self.graph.as_ref()?;

        // Create ray from input arrays
        let ray = Ray::new(
            Vector3::new(ray_origin[0], ray_origin[1], ray_origin[2]),
            Vector3::new(ray_direction[0], ray_direction[1], ray_direction[2]),
        );

        // Default node radius for collision detection
        // In a real implementation, this could be configurable or per-node
        let node_radius = 20.0; // Matches typical visual size

        // Convert graph nodes to hit test format
        let nodes: Vec<NodeHitTest> = graph
            .nodes()
            .iter()
            .map(|node| NodeHitTest {
                id: node.id.clone(),
                position: Vector3::new(
                    node.visual.x as f32,
                    node.visual.y as f32,
                    0.0, // Z=0 for 2D layout, can be extended for 3D
                ),
                radius: node_radius,
            })
            .collect();

        // Find closest hit
        find_closest_node_hit(&ray, &nodes).map(|(id, _distance)| id)
    }

    /// Pin a node at a specific position
    ///
    /// Pinned nodes will not be moved by the force simulation.
    /// This is useful for allowing users to fix nodes in place.
    ///
    /// # Arguments
    /// * `node_id` - The ID of the node to pin
    /// * `x` - X coordinate to pin the node at
    /// * `y` - Y coordinate to pin the node at
    ///
    /// # Returns
    /// * `Ok(())` if the node was successfully pinned
    /// * `Err` if the node doesn't exist
    ///
    /// # Example (from JavaScript)
    /// ```javascript
    /// // Pin a node at position (100, 200)
    /// webvowl.pinNode("node_1", 100, 200);
    ///
    /// // Run simulation - this node will stay at (100, 200)
    /// webvowl.runSimulation(50);
    /// ```
    #[wasm_bindgen(js_name = pinNode)]
    pub fn pin_node(&mut self, node_id: &str, x: f64, y: f64) -> std::result::Result<(), JsValue> {
        let graph = self
            .graph
            .as_mut()
            .ok_or_else(|| JsValue::from_str("No graph loaded"))?;

        self.pin_manager
            .pin_node_at(graph, node_id, x, y)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Unpin a node, allowing it to move freely
    ///
    /// # Arguments
    /// * `node_id` - The ID of the node to unpin
    ///
    /// # Returns
    /// * `Ok(())` if the node was successfully unpinned
    /// * `Err` if the node doesn't exist
    ///
    /// # Example (from JavaScript)
    /// ```javascript
    /// // Unpin a previously pinned node
    /// webvowl.unpinNode("node_1");
    /// ```
    #[wasm_bindgen(js_name = unpinNode)]
    pub fn unpin_node(&mut self, node_id: &str) -> std::result::Result<(), JsValue> {
        let graph = self
            .graph
            .as_mut()
            .ok_or_else(|| JsValue::from_str("No graph loaded"))?;

        self.pin_manager
            .unpin_node(graph, node_id)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Check if a node is currently pinned
    ///
    /// # Arguments
    /// * `node_id` - The ID of the node to check
    ///
    /// # Returns
    /// * `true` if the node is pinned
    /// * `false` if the node is not pinned or doesn't exist
    ///
    /// # Example (from JavaScript)
    /// ```javascript
    /// if (webvowl.isPinned("node_1")) {
    ///     console.log("Node is pinned");
    /// }
    /// ```
    #[wasm_bindgen(js_name = isPinned)]
    pub fn is_pinned(&self, node_id: &str) -> bool {
        self.pin_manager.is_pinned(node_id)
    }

    /// Unpin all nodes
    ///
    /// Removes all pins, allowing all nodes to move freely again.
    ///
    /// # Example (from JavaScript)
    /// ```javascript
    /// // Reset all pinned nodes
    /// webvowl.resetPins();
    /// ```
    #[wasm_bindgen(js_name = resetPins)]
    pub fn reset_pins(&mut self) -> std::result::Result<(), JsValue> {
        let graph = self
            .graph
            .as_mut()
            .ok_or_else(|| JsValue::from_str("No graph loaded"))?;

        self.pin_manager.reset(graph);
        Ok(())
    }

    /// Get the number of currently pinned nodes
    ///
    /// # Returns
    /// * The count of pinned nodes
    ///
    /// # Example (from JavaScript)
    /// ```javascript
    /// const count = webvowl.getPinnedCount();
    /// console.log(`${count} nodes are pinned`);
    /// ```
    #[wasm_bindgen(js_name = getPinnedCount)]
    pub fn get_pinned_count(&self) -> usize {
        self.pin_manager.pinned_count()
    }

    /// Filter nodes by minimum degree
    ///
    /// Filters the graph to show only nodes with at least the specified degree.
    /// Degree is calculated excluding datatype connections, focusing on structural
    /// relationships.
    ///
    /// # Arguments
    /// * `min_degree` - Minimum number of non-datatype connections required
    ///
    /// # Returns
    /// * `Ok(())` if filtering succeeded
    /// * `Err` if graph not loaded
    ///
    /// # Example (from JavaScript)
    /// ```javascript
    /// // Show only nodes with 3+ connections
    /// webvowl.filterByDegree(3);
    /// ```
    #[wasm_bindgen(js_name = filterByDegree)]
    pub fn filter_by_degree(&mut self, min_degree: usize) -> std::result::Result<(), JsValue> {
        let graph = self
            .graph
            .as_mut()
            .ok_or_else(|| JsValue::from_str("No graph loaded"))?;

        graph
            .filter_by_degree(min_degree)
            .map_err(|e| JsValue::from_str(&format!("Filter error: {}", e)))
    }

    /// Toggle visibility of datatype nodes
    ///
    /// Show or hide all datatype nodes (e.g., xsd:string, xsd:int).
    /// This is useful for simplifying the visualization by hiding literal types.
    ///
    /// # Arguments
    /// * `visible` - true to show datatypes, false to hide them
    ///
    /// # Returns
    /// * `Ok(())` if toggle succeeded
    /// * `Err` if graph not loaded
    ///
    /// # Example (from JavaScript)
    /// ```javascript
    /// // Hide all datatype nodes
    /// webvowl.toggleDatatypes(false);
    /// ```
    #[wasm_bindgen(js_name = toggleDatatypes)]
    pub fn toggle_datatypes(&mut self, visible: bool) -> std::result::Result<(), JsValue> {
        let graph = self
            .graph
            .as_mut()
            .ok_or_else(|| JsValue::from_str("No graph loaded"))?;

        graph
            .toggle_datatypes(visible)
            .map_err(|e| JsValue::from_str(&format!("Filter error: {}", e)))
    }

    /// Toggle visibility of set operator nodes
    ///
    /// Show or hide set operator nodes (union, intersection, complement).
    /// These represent complex class expressions in OWL ontologies.
    ///
    /// # Arguments
    /// * `visible` - true to show operators, false to hide them
    ///
    /// # Returns
    /// * `Ok(())` if toggle succeeded
    /// * `Err` if graph not loaded
    ///
    /// # Example (from JavaScript)
    /// ```javascript
    /// // Hide set operators for simpler view
    /// webvowl.toggleSetOperators(false);
    /// ```
    #[wasm_bindgen(js_name = toggleSetOperators)]
    pub fn toggle_set_operators(&mut self, visible: bool) -> std::result::Result<(), JsValue> {
        let graph = self
            .graph
            .as_mut()
            .ok_or_else(|| JsValue::from_str("No graph loaded"))?;

        graph
            .toggle_set_operators(visible)
            .map_err(|e| JsValue::from_str(&format!("Filter error: {}", e)))
    }

    /// Reset all visibility filters
    ///
    /// Makes all nodes visible again, clearing any filters that were applied.
    ///
    /// # Returns
    /// * `Ok(())` if reset succeeded
    /// * `Err` if graph not loaded
    ///
    /// # Example (from JavaScript)
    /// ```javascript
    /// // Clear all filters
    /// webvowl.resetVisibility();
    /// ```
    #[wasm_bindgen(js_name = resetVisibility)]
    pub fn reset_visibility(&mut self) -> std::result::Result<(), JsValue> {
        let graph = self
            .graph
            .as_mut()
            .ok_or_else(|| JsValue::from_str("No graph loaded"))?;

        graph
            .reset_visibility()
            .map_err(|e| JsValue::from_str(&format!("Filter error: {}", e)))
    }

    /// Get node degree (excluding datatypes)
    ///
    /// Returns the number of non-datatype connections for a specific node.
    /// This is useful for understanding node importance in the ontology structure.
    ///
    /// # Arguments
    /// * `node_id` - The ID of the node
    ///
    /// # Returns
    /// * Number of non-datatype connections
    /// * `Err` if node doesn't exist or graph not loaded
    ///
    /// # Example (from JavaScript)
    /// ```javascript
    /// const degree = webvowl.getNodeDegree("class_1");
    /// console.log(`Node has ${degree} connections`);
    /// ```
    #[wasm_bindgen(js_name = getNodeDegree)]
    pub fn get_node_degree(&self, node_id: &str) -> std::result::Result<usize, JsValue> {
        let graph = self
            .graph
            .as_ref()
            .ok_or_else(|| JsValue::from_str("No graph loaded"))?;

        graph
            .non_datatype_degree(node_id)
            .map_err(|e| JsValue::from_str(&format!("Degree error: {}", e)))
    }
}

/// Graph data for JSON export
#[cfg(feature = "debug-serde")]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct GraphData {
    nodes: Vec<NodeData>,
    edges: Vec<EdgeData>,
}

#[cfg(feature = "debug-serde")]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct NodeData {
    id: String,
    label: String,
    x: f64,
    y: f64,
    node_type: String,
    visible: bool,
    term_id: Option<String>, // For linking to published pages
    iri: Option<String>,     // Full ontology IRI
}

#[cfg(feature = "debug-serde")]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct EdgeData {
    id: String,
    label: String,
    source: String,
    target: String,
    edge_type: String,
    inverse_of: Option<String>,
    equivalent_to: Vec<String>,
    disjoint_with: Vec<String>,
}

#[cfg(feature = "debug-serde")]
impl GraphData {
    fn from_graph(graph: &VowlGraph) -> Self {
        let nodes = graph
            .nodes()
            .iter()
            .map(|n| NodeData {
                id: n.id.clone(),
                label: n.label.clone(),
                x: n.visual.x,
                y: n.visual.y,
                node_type: format!("{:?}", n.node_type),
                visible: n.visual.visible,
                term_id: n.semantic.ontology_meta.as_ref().map(|m| m.term_id.clone()),
                iri: Some(n.semantic.iri.clone()),
            })
            .collect();

        // ✅ FIX: Extract edges with proper source/target from graph topology
        // AND filter out edges connected to invisible nodes
        let edges = graph
            .edges_with_endpoints()
            .iter()
            .filter(|(_, source, target)| source.visual.visible && target.visual.visible)
            .map(|(edge, source, target)| EdgeData {
                id: edge.id.clone(),
                label: edge.label.clone(),
                source: source.id.clone(),
                target: target.id.clone(),
                edge_type: format!("{:?}", edge.edge_type),
                inverse_of: edge.inverse_of.clone(),
                equivalent_to: edge.equivalent_to.clone(),
                disjoint_with: edge.disjoint_with.clone(),
            })
            .collect();

        Self { nodes, edges }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test;

    // Run under `cargo test` on the host and under `wasm-pack test` in a
    // browser: bare `#[wasm_bindgen_test]` expands to nothing off wasm32.
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    fn test_webvowl_creation() {
        let webvowl = WebVowl::new();
        assert_eq!(webvowl.get_node_count(), 0);
        assert_eq!(webvowl.get_edge_count(), 0);
    }

    // Run under `cargo test` on the host and under `wasm-pack test` in a
    // browser: bare `#[wasm_bindgen_test]` expands to nothing off wasm32.
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    fn test_load_ontology() {
        let mut webvowl = WebVowl::new();

        let json = r#"
        {
            "class": [
                {
                    "id": "class1",
                    "label": "Class 1",
                    "type": "owl:Class"
                }
            ],
            "property": []
        }
        "#;

        let result = webvowl.load_ontology(json);
        assert!(result.is_ok());
        assert_eq!(webvowl.get_node_count(), 1);
    }

    #[test]
    fn loading_a_new_ontology_discards_old_pins_and_cooling() {
        let mut vowl = WebVowl::new();
        vowl.load_ontology(TWO_CLASS_ONTOLOGY).unwrap();
        let id = vowl.graph.as_ref().unwrap().nodes()[0].id.clone();
        vowl.pin_manager
            .pin_node(vowl.graph.as_mut().unwrap(), &id)
            .unwrap();
        vowl.simulation.reheat(0.01);
        vowl.load_ontology(TWO_CLASS_ONTOLOGY).unwrap();
        assert!(!vowl.pin_manager.is_pinned(&id));
        assert_eq!(vowl.simulation.alpha(), 1.0);
        assert!(!vowl.graph.as_ref().unwrap().nodes()[0].visual.fixed);
    }

    /// A two-class, one-property ontology; enough to have a stable node order.
    const TWO_CLASS_ONTOLOGY: &str = r#"{
        "class": [
            { "id": "c1", "label": "Person",       "type": "owl:Class" },
            { "id": "c2", "label": "Organisation", "type": "owl:Class" }
        ],
        "property": [
            { "id": "p1", "label": "worksFor", "type": "owl:ObjectProperty",
              "domain": "c1", "range": "c2" }
        ]
    }"#;

    // Run under `cargo test` on the host and under `wasm-pack test` in a
    // browser: bare `#[wasm_bindgen_test]` expands to nothing off wasm32.
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    fn positions_are_empty_before_an_ontology_is_loaded() {
        let vowl = WebVowl::new();
        assert!(vowl.get_node_positions().is_empty());
        assert!(vowl.get_node_ids().is_empty());

        let mut buf = [1.0f32; 4];
        assert_eq!(vowl.write_positions_into(&mut buf), 0);
        assert_eq!(buf, [1.0; 4], "an unloaded graph must not touch the buffer");
    }

    // Run under `cargo test` on the host and under `wasm-pack test` in a
    // browser: bare `#[wasm_bindgen_test]` expands to nothing off wasm32.
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    fn positions_are_two_floats_per_node() {
        let mut vowl = WebVowl::new();
        vowl.load_ontology(TWO_CLASS_ONTOLOGY).unwrap();

        assert_eq!(vowl.get_node_count(), 2);
        assert_eq!(vowl.get_node_positions().len(), vowl.get_node_count() * 2);
        assert_eq!(vowl.get_node_ids().len(), vowl.get_node_count());
    }

    // Run under `cargo test` on the host and under `wasm-pack test` in a
    // browser: bare `#[wasm_bindgen_test]` expands to nothing off wasm32.
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    fn positions_are_indexed_by_the_id_order() {
        let mut vowl = WebVowl::new();
        vowl.load_ontology(TWO_CLASS_ONTOLOGY).unwrap();

        // Pin each node somewhere unmistakable, then read the positions back
        // through the index: `ids[i]` must describe `xy[2i], xy[2i + 1]`.
        vowl.pin_node("c1", 10.0, 20.0).unwrap();
        vowl.pin_node("c2", -30.0, 40.0).unwrap();

        let ids = vowl.get_node_ids();
        let xy = vowl.get_node_positions();

        for (i, id) in ids.iter().enumerate() {
            let expected = match id.as_str() {
                "c1" => (10.0, 20.0),
                "c2" => (-30.0, 40.0),
                other => panic!("unexpected node id {other}"),
            };
            assert_eq!(
                (xy[2 * i], xy[2 * i + 1]),
                expected,
                "node {id} at index {i}"
            );
        }
    }

    // Run under `cargo test` on the host and under `wasm-pack test` in a
    // browser: bare `#[wasm_bindgen_test]` expands to nothing off wasm32.
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    fn the_index_order_survives_ticking_and_filtering() {
        let mut vowl = WebVowl::new();
        vowl.load_ontology(TWO_CLASS_ONTOLOGY).unwrap();
        let before = vowl.get_node_ids();

        vowl.init_simulation().unwrap();
        vowl.run_simulation(25).unwrap();
        assert_eq!(
            vowl.get_node_ids(),
            before,
            "ticking must not reorder nodes"
        );

        // Filtering flips a visibility flag; it must not drop a node from the
        // index, or every captured index would silently shift.
        vowl.filter_by_degree(99).unwrap();
        assert_eq!(
            vowl.get_node_ids(),
            before,
            "filtering must not reorder nodes"
        );
        assert_eq!(vowl.get_node_positions().len(), before.len() * 2);

        vowl.reset_visibility().unwrap();
        assert_eq!(vowl.get_node_ids(), before);
    }

    // Run under `cargo test` on the host and under `wasm-pack test` in a
    // browser: bare `#[wasm_bindgen_test]` expands to nothing off wasm32.
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    fn write_positions_into_matches_the_allocating_accessor() {
        let mut vowl = WebVowl::new();
        vowl.load_ontology(TWO_CLASS_ONTOLOGY).unwrap();
        vowl.init_simulation().unwrap();
        vowl.run_simulation(10).unwrap();

        let allocated = vowl.get_node_positions();
        let mut buf = vec![0.0f32; allocated.len()];
        assert_eq!(vowl.write_positions_into(&mut buf), allocated.len());
        assert_eq!(buf, allocated);
    }

    // Run under `cargo test` on the host and under `wasm-pack test` in a
    // browser: bare `#[wasm_bindgen_test]` expands to nothing off wasm32.
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    fn write_positions_into_writes_only_the_overlap() {
        let mut vowl = WebVowl::new();
        vowl.load_ontology(TWO_CLASS_ONTOLOGY).unwrap();
        vowl.pin_node("c1", 1.0, 2.0).unwrap();

        let full = vowl.get_node_positions();
        assert_eq!(full.len(), 4);

        // Short buffer: whole nodes only, truncated rather than failing.
        let mut short = [-1.0f32; 3];
        assert_eq!(vowl.write_positions_into(&mut short), 2);
        assert_eq!([short[0], short[1]], [full[0], full[1]]);
        assert_eq!(short[2], -1.0, "a partial node must not be written");

        // Long buffer: the tail is left alone.
        let mut long = [-1.0f32; 6];
        assert_eq!(vowl.write_positions_into(&mut long), 4);
        assert_eq!(long[..4], full[..4]);
        assert_eq!([long[4], long[5]], [-1.0, -1.0]);
    }
}
