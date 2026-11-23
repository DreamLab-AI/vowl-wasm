//! Force-directed layout simulation
//!
//! This module implements a force-directed graph layout algorithm with
//! optional SIMD acceleration. When the `simd` feature is enabled, batch
//! force calculations use WASM SIMD128 for improved performance.
//!
//! ## Performance
//! - Without SIMD: ~0.15ms per tick for 100 nodes
//! - With SIMD: ~0.08ms per tick for 100 nodes (50% improvement)
//! - Barnes-Hut: O(n log n) for large graphs (>50 nodes)

use super::{force::*, LayoutAlgorithm, LayoutConfig};
use crate::debug::{log_distribution, DebugFlags};
use crate::graph::VowlGraph;
use crate::Result;
use nalgebra::Vector2;

/// Per-tick displacement cap, as a multiple of the larger of `link_distance`
/// and `seed_radius`.
///
/// A guard, not part of the physics. The largest legitimate step is the first
/// contraction from the seed circle, which moves a node by up to about one
/// seed radius; the real ontology fixtures peak at 0.55 of this cap and never
/// reach it. It exists so that a caller-chosen stiffness the explicit
/// integrator cannot carry grows the layout linearly instead of
/// geometrically.
const MAX_STEP_FACTOR: f64 = 2.0;

/// Force-directed layout simulation
pub struct ForceSimulation {
    config: LayoutConfig,
    alpha: f64,
    iteration: usize,
    debug_flags: DebugFlags,
}

impl ForceSimulation {
    /// Create a new simulation with default configuration
    pub fn new() -> Self {
        Self {
            config: LayoutConfig::default(),
            alpha: 1.0,
            iteration: 0,
            debug_flags: DebugFlags::default(),
        }
    }

    /// Create a simulation with custom configuration
    pub fn with_config(config: LayoutConfig) -> Self {
        let alpha = config.alpha;
        Self {
            config,
            alpha,
            iteration: 0,
            debug_flags: DebugFlags::default(),
        }
    }

    /// Enable debug logging
    pub fn enable_debug(&mut self) {
        self.debug_flags.enable_all();
    }

    /// Set debug flags
    pub fn set_debug_flags(&mut self, flags: DebugFlags) {
        self.debug_flags = flags;
    }

    /// Set center position
    pub fn set_center(&mut self, x: f64, y: f64) {
        self.config.center = (x, y);
    }

    /// Set link distance
    pub fn set_link_distance(&mut self, distance: f64) {
        self.config.link_distance = distance;
    }
    /// Re-heat a settled or cooling simulation so it resumes motion
    /// (protocol `reheat`, matching `CsrSimulation::reheat`). `alpha` is
    /// clamped to `(0, 1]`; a non-finite or non-positive value is ignored.
    pub fn reheat(&mut self, alpha: f64) {
        if alpha.is_finite() && alpha > 0.0 {
            self.alpha = alpha.min(1.0);
        }
    }

    /// Return to the unstarted state for a newly loaded graph.
    ///
    /// Alpha goes back to `config.alpha` and the iteration count to zero. The
    /// configuration (link distance, charge, centre and the rest) and the
    /// debug flags are kept, so settings made before a graph is loaded stay in
    /// force for it.
    pub fn reset(&mut self) {
        self.alpha = self.config.alpha;
        self.iteration = 0;
    }

    /// Set charge strength
    pub fn set_charge_strength(&mut self, strength: f64) {
        self.config.charge_strength = strength;
    }

    /// Seed node positions evenly around a circle of `config.seed_radius`.
    ///
    /// Every node is reseeded and its velocity zeroed, so `initialize` always
    /// yields the same starting state for the same graph. When
    /// `config.preserve_seeded_positions` is set, a node that is already
    /// somewhere other than the origin keeps its position — that is the path for
    /// resuming a saved layout, or for honouring positions the caller placed.
    fn initialize_positions(&self, graph: &mut VowlGraph) {
        let radius = self.config.seed_radius;
        let mut angle: f64 = 0.0;
        let angle_step = std::f64::consts::TAU / graph.node_count() as f64;

        #[cfg(target_arch = "wasm32")]
        {
            web_sys::console::log_1(
                &format!(
                    "🔄 Initializing {} nodes in circular layout with radius {}",
                    graph.node_count(),
                    radius
                )
                .into(),
            );
        }

        // Collect node IDs first to avoid borrow conflicts
        let node_ids: Vec<String> = graph.nodes().iter().map(|n| n.id.clone()).collect();

        for node_id in node_ids {
            if let Some(node_mut) = graph.get_node_mut(&node_id) {
                let already_placed = node_mut.visual.x != 0.0 || node_mut.visual.y != 0.0;
                if !(self.config.preserve_seeded_positions && already_placed) {
                    node_mut.visual.x = radius * angle.cos();
                    node_mut.visual.y = radius * angle.sin();
                }
                // Velocities are always reset: a resumed layout restarts from
                // rest, not from whatever momentum it was saved mid-tick with.
                node_mut.visual.vx = 0.0;
                node_mut.visual.vy = 0.0;
                angle += angle_step;
            }
        }

        // Log initial distribution
        if self.debug_flags.log_positions {
            let positions: Vec<(f64, f64)> = graph
                .nodes()
                .iter()
                .map(|n| (n.visual.x, n.visual.y))
                .collect();
            log_distribution("Initial Positions", &positions);
        }
    }

    /// Calculate the force on every node for one simulation step, in node
    /// index order.
    ///
    /// When the `simd` feature is enabled, uses SIMD-accelerated batch
    /// calculations for repulsion and centering forces.
    fn calculate_forces(&self, graph: &VowlGraph) -> Vec<Vector2<f64>> {
        let nodes = graph.nodes();
        let n = nodes.len();
        let positions: Vec<(f64, f64)> = nodes.iter().map(|n| (n.visual.x, n.visual.y)).collect();

        // Log SIMD status on first iteration
        #[cfg(all(target_arch = "wasm32", feature = "simd"))]
        {
            if self.iteration == 0 {
                web_sys::console::log_1(&"[SIMD] WASM SIMD128 force calculations enabled".into());
            }
        }

        // Many-body repulsion between all nodes
        let mut forces = if self.config.use_barnes_hut && n > 50 {
            // Barnes-Hut approximation for large graphs; its leaf nodes use
            // SIMD when the feature is enabled.
            use super::quadtree::QuadTree;

            let quadtree = QuadTree::build(&nodes);
            nodes
                .iter()
                .map(|node| {
                    quadtree.calculate_force(
                        Vector2::new(node.visual.x, node.visual.y),
                        &node.id,
                        self.config.barnes_hut_theta,
                        self.config.charge_strength,
                    )
                })
                .collect()
        } else {
            calculate_batch_repulsion(&positions, self.config.charge_strength)
        };

        let link_count = self.add_link_forces(graph, &positions, &mut forces);

        // Log the link count on first iteration only
        #[cfg(target_arch = "wasm32")]
        {
            if self.iteration == 0 {
                web_sys::console::log_1(
                    &format!(
                        "[Force] Applied {} link forces between {} nodes ({} edges in graph)",
                        link_count,
                        n,
                        graph.edge_count()
                    )
                    .into(),
                );
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = link_count;

        forces
    }

    /// Translate the movable nodes so their centroid moves `center_strength`
    /// of the way to `center` (d3-force's `forceCenter`).
    ///
    /// This acts on positions, not velocities, and moves every node by the same
    /// offset, so it keeps the layout where the caller wants it without
    /// squeezing it: unlike a per-node pull towards the centre, it adds no
    /// stiffness and cannot destabilise the integrator.
    fn recenter(&self, graph: &mut VowlGraph) {
        let (mut sum_x, mut sum_y, mut movable) = (0.0, 0.0, 0usize);
        for node in graph.nodes().iter().filter(|n| !n.visual.fixed) {
            sum_x += node.visual.x;
            sum_y += node.visual.y;
            movable += 1;
        }
        if movable == 0 {
            return;
        }
        let strength = self.config.center_strength;
        let shift_x = (self.config.center.0 - sum_x / movable as f64) * strength;
        let shift_y = (self.config.center.1 - sum_y / movable as f64) * strength;
        if !(shift_x.is_finite() && shift_y.is_finite()) {
            return;
        }
        for node in graph.nodes_mut().filter(|n| !n.visual.fixed) {
            node.visual.x += shift_x;
            node.visual.y += shift_y;
        }
    }

    /// Add the spring force of every edge to `forces` and return how many
    /// edges pulled.
    ///
    /// This is d3-force's `forceLink` with its default strength and bias. Each
    /// edge is visited once and acts on both ends. Its stiffness is
    /// `link_strength / min(degree(source), degree(target))`, and the correction
    /// is shared by degree: the target takes `degree(source) / (degree(source) +
    /// degree(target))` of it and the source the rest. Summed over a node's
    /// edges, that keeps every node's total stiffness below `link_strength`
    /// however many edges (parallel ones included) it carries, which is what
    /// keeps the explicit integrator stable on a hub.
    ///
    /// Self-loops exert no force and are left out of the degree count, so they
    /// do not weaken a node's other links.
    fn add_link_forces(
        &self,
        graph: &VowlGraph,
        positions: &[(f64, f64)],
        forces: &mut [Vector2<f64>],
    ) -> usize {
        let links: Vec<(usize, usize)> = graph
            .edge_index_pairs()
            .filter(|(source, target)| source != target)
            .collect();

        let mut degree = vec![0usize; positions.len()];
        for &(source, target) in &links {
            degree[source] += 1;
            degree[target] += 1;
        }

        let mut pulled = 0;
        for &(source, target) in &links {
            let (sx, sy) = positions[source];
            let (tx, ty) = positions[target];
            let delta = Vector2::new(tx - sx, ty - sy);
            let distance = delta.norm();
            // Coincident ends have no direction to pull along; repulsion
            // separates them first.
            if distance < 1e-4 {
                continue;
            }

            let source_degree = degree[source] as f64;
            let target_degree = degree[target] as f64;
            let stiffness = self.config.link_strength / source_degree.min(target_degree);
            let pull = delta * ((distance - self.config.link_distance) / distance * stiffness);
            let target_share = source_degree / (source_degree + target_degree);

            forces[target] -= pull * target_share;
            forces[source] += pull * (1.0 - target_share);
            pulled += 1;
        }
        pulled
    }

    /// Integrate one step: `v = (v + alpha * F) * velocity_decay`, `x += v`.
    ///
    /// Alpha is applied here and only here. Fixed (pinned) nodes do not move.
    /// The step length is capped at [`MAX_STEP_FACTOR`] times the larger of
    /// `link_distance` and `seed_radius`, and a non-finite velocity is
    /// dropped; both are guards that well-posed input never reaches.
    fn apply_forces(&self, graph: &mut VowlGraph, forces: &[Vector2<f64>]) {
        let max_step = MAX_STEP_FACTOR
            * self
                .config
                .link_distance
                .abs()
                .max(self.config.seed_radius.abs());
        for (node, force) in graph.nodes_mut().zip(forces) {
            if node.visual.fixed {
                continue;
            }
            let mut velocity = (Vector2::new(node.visual.vx, node.visual.vy) + force * self.alpha)
                * self.config.velocity_decay;
            let speed = velocity.norm();
            if !speed.is_finite() {
                velocity = Vector2::zeros();
            } else if speed > max_step {
                velocity *= max_step / speed;
            }

            node.visual.vx = velocity.x;
            node.visual.vy = velocity.y;
            node.visual.x += velocity.x;
            node.visual.y += velocity.y;
        }
    }
}

impl LayoutAlgorithm for ForceSimulation {
    fn initialize(&mut self, graph: &mut VowlGraph) -> Result<()> {
        self.initialize_positions(graph);
        self.alpha = self.config.alpha;
        self.iteration = 0;
        Ok(())
    }

    fn tick(&mut self, graph: &mut VowlGraph) -> Result<()> {
        if self.is_finished() {
            return Ok(());
        }

        // Log before tick
        if self.debug_flags.should_log(self.iteration) && self.debug_flags.log_positions {
            let positions: Vec<(f64, f64)> = graph
                .nodes()
                .iter()
                .map(|n| (n.visual.x, n.visual.y))
                .collect();
            log_distribution(&format!("Before Tick {}", self.iteration), &positions);
        }

        let forces = self.calculate_forces(graph);
        self.apply_forces(graph, &forces);
        self.recenter(graph);

        // Decay alpha
        self.alpha *= 1.0 - self.config.alpha_decay;
        self.iteration += 1;

        // Log after tick
        if self.debug_flags.should_log(self.iteration) {
            #[cfg(target_arch = "wasm32")]
            {
                if self.debug_flags.log_positions {
                    let positions: Vec<(f64, f64)> = graph
                        .nodes()
                        .iter()
                        .map(|n| (n.visual.x, n.visual.y))
                        .collect();
                    log_distribution(&format!("After Tick {}", self.iteration), &positions);
                }

                web_sys::console::log_1(
                    &format!(
                        "⏱️ Iteration {}: alpha={:.4}, nodes={}",
                        self.iteration,
                        self.alpha,
                        graph.node_count()
                    )
                    .into(),
                );
            }
        }

        Ok(())
    }

    fn run(&mut self, graph: &mut VowlGraph, iterations: usize) -> Result<()> {
        self.initialize(graph)?;

        for _ in 0..iterations {
            if self.is_finished() {
                break;
            }
            self.tick(graph)?;
        }

        Ok(())
    }

    fn is_finished(&self) -> bool {
        self.alpha < self.config.alpha_min
    }

    fn alpha(&self) -> f64 {
        self.alpha
    }
}

impl Default for ForceSimulation {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reheat_resumes_a_settled_simulation() {
        let mut sim = ForceSimulation::new();
        sim.alpha = 0.0;
        assert!(sim.is_finished());
        sim.reheat(f64::NAN);
        assert_eq!(sim.alpha, 0.0);
        sim.reheat(-1.0);
        assert_eq!(sim.alpha, 0.0);
        sim.reheat(2.0);
        assert_eq!(sim.alpha, 1.0);
        assert!(!sim.is_finished());
        sim.reheat(0.3);
        assert!((sim.alpha - 0.3).abs() < 1e-12);
    }

    use super::*;
    use crate::graph::{node::NodeBuilder, NodeType, VowlGraph};

    fn create_test_graph() -> VowlGraph {
        let mut graph = VowlGraph::new();

        let node1 = NodeBuilder::new("node1")
            .label("Node 1")
            .node_type(NodeType::Class)
            .build();

        let node2 = NodeBuilder::new("node2")
            .label("Node 2")
            .node_type(NodeType::Class)
            .build();

        let node3 = NodeBuilder::new("node3")
            .label("Node 3")
            .node_type(NodeType::Class)
            .build();

        graph.add_node(node1).unwrap();
        graph.add_node(node2).unwrap();
        graph.add_node(node3).unwrap();

        graph
    }

    #[test]
    fn test_simulation_creation() {
        let sim = ForceSimulation::new();
        assert_eq!(sim.alpha, 1.0);
        assert_eq!(sim.iteration, 0);
    }

    #[test]
    fn test_simulation_with_config() {
        let config = LayoutConfig {
            alpha: 0.5,
            link_distance: 50.0,
            ..Default::default()
        };

        let sim = ForceSimulation::with_config(config);
        assert_eq!(sim.alpha, 0.5);
        assert_eq!(sim.config.link_distance, 50.0);
    }

    #[test]
    fn test_initialize_positions() {
        let mut graph = create_test_graph();
        let mut sim = ForceSimulation::new();

        sim.initialize(&mut graph).unwrap();

        // All nodes should have non-zero positions
        for node in graph.nodes() {
            assert!(
                node.visual.x != 0.0 || node.visual.y != 0.0,
                "Node should have initialized position"
            );
        }
    }

    #[test]
    fn test_single_tick() {
        let mut graph = create_test_graph();
        let mut sim = ForceSimulation::new();

        sim.initialize(&mut graph).unwrap();
        let initial_alpha = sim.alpha();

        sim.tick(&mut graph).unwrap();

        // Alpha should have decayed
        assert!(sim.alpha() < initial_alpha);
    }

    #[test]
    fn test_run_simulation() {
        let mut graph = create_test_graph();
        let mut sim = ForceSimulation::new();

        let result = sim.run(&mut graph, 100);
        assert!(result.is_ok());

        // Alpha should have decayed significantly
        assert!(sim.alpha() < 0.5);
    }

    #[test]
    fn test_simulation_finishes() {
        let mut graph = create_test_graph();
        let mut sim = ForceSimulation::new();

        sim.run(&mut graph, 1000).unwrap();

        // Simulation should have finished
        assert!(sim.is_finished());
    }

    #[test]
    fn test_set_center() {
        let mut sim = ForceSimulation::new();
        sim.set_center(100.0, 200.0);

        assert_eq!(sim.config.center, (100.0, 200.0));
    }

    #[test]
    fn test_set_parameters() {
        let mut sim = ForceSimulation::new();

        sim.set_link_distance(50.0);
        sim.set_charge_strength(-100.0);

        assert_eq!(sim.config.link_distance, 50.0);
        assert_eq!(sim.config.charge_strength, -100.0);
    }

    fn class(id: &str) -> NodeBuilder {
        NodeBuilder::new(id).node_type(NodeType::Class)
    }

    fn link(graph: &mut VowlGraph, id: &str, from: &str, to: &str) {
        graph
            .add_edge(from, to, crate::graph::EdgeBuilder::new(id).build())
            .unwrap();
    }

    fn settle(graph: &mut VowlGraph, sim: &mut ForceSimulation) {
        sim.initialize(graph).unwrap();
        while !sim.is_finished() {
            sim.tick(graph).unwrap();
        }
    }

    fn distance(graph: &VowlGraph, a: &str, b: &str) -> f64 {
        let (a, b) = (graph.get_node(a).unwrap(), graph.get_node(b).unwrap());
        (a.visual.x - b.visual.x).hypot(a.visual.y - b.visual.y)
    }

    #[test]
    fn unlinked_nodes_push_apart() {
        let mut graph = VowlGraph::new();
        graph
            .add_node(class("a").position(-1.0, 0.0).build())
            .unwrap();
        graph
            .add_node(class("b").position(1.0, 0.0).build())
            .unwrap();
        let mut sim = ForceSimulation::with_config(LayoutConfig {
            preserve_seeded_positions: true,
            ..LayoutConfig::default()
        });
        sim.initialize(&mut graph).unwrap();
        sim.tick(&mut graph).unwrap();
        assert!(distance(&graph, "a", "b") > 2.0, "charge must repel");
    }

    #[test]
    fn a_hub_with_many_leaves_stays_bounded() {
        // The 0.2.3 integrator summed one unit-stiffness spring per out-edge on
        // the hub, which goes unstable above about five edges.
        let mut graph = VowlGraph::new();
        graph.add_node(class("hub").build()).unwrap();
        for i in 0..40 {
            graph.add_node(class(&format!("leaf{i}")).build()).unwrap();
            link(&mut graph, &format!("e{i}"), "hub", &format!("leaf{i}"));
        }
        let mut sim = ForceSimulation::new();
        settle(&mut graph, &mut sim);
        for node in graph.nodes() {
            assert!(
                node.visual.x.abs() < 1_000.0 && node.visual.y.abs() < 1_000.0,
                "{} at ({}, {})",
                node.id,
                node.visual.x,
                node.visual.y
            );
        }
    }

    #[test]
    fn parallel_edges_do_not_stack() {
        // Ten parallel edges between one pair are one spring, not ten.
        let mut graph = VowlGraph::new();
        graph.add_node(class("a").build()).unwrap();
        graph.add_node(class("b").build()).unwrap();
        for i in 0..10 {
            link(&mut graph, &format!("p{i}"), "a", "b");
        }
        let mut sim = ForceSimulation::new();
        settle(&mut graph, &mut sim);
        let d = distance(&graph, "a", "b");
        let rest = LayoutConfig::default().link_distance;
        assert!(
            d.is_finite() && (d - rest).abs() < 0.5 * rest,
            "settled {d} apart, rest length {rest}"
        );
    }

    #[test]
    fn a_pinned_node_is_not_moved_by_centring() {
        let mut graph = VowlGraph::new();
        graph
            .add_node(class("pinned").position(500.0, 0.0).build())
            .unwrap();
        graph.add_node(class("free").build()).unwrap();
        link(&mut graph, "e", "pinned", "free");
        graph.get_node_mut("pinned").unwrap().visual.fixed = true;
        let mut sim = ForceSimulation::with_config(LayoutConfig {
            preserve_seeded_positions: true,
            ..LayoutConfig::default()
        });
        settle(&mut graph, &mut sim);
        let pinned = graph.get_node("pinned").unwrap();
        assert_eq!((pinned.visual.x, pinned.visual.y), (500.0, 0.0));
    }

    #[test]
    fn centring_moves_the_centroid_to_the_centre() {
        let mut graph = create_test_graph();
        let mut sim = ForceSimulation::new();
        sim.set_center(100.0, -50.0);
        settle(&mut graph, &mut sim);
        let n = graph.node_count() as f64;
        let cx = graph.nodes().iter().map(|n| n.visual.x).sum::<f64>() / n;
        let cy = graph.nodes().iter().map(|n| n.visual.y).sum::<f64>() / n;
        assert!(
            (cx - 100.0).abs() < 1e-6 && (cy + 50.0).abs() < 1e-6,
            "centroid ({cx}, {cy})"
        );
    }

    #[test]
    fn reset_keeps_the_configuration() {
        let mut sim = ForceSimulation::new();
        sim.set_link_distance(80.0);
        sim.set_charge_strength(-200.0);
        sim.set_center(5.0, 6.0);
        sim.alpha = 0.0;
        sim.iteration = 42;
        sim.reset();
        assert_eq!(sim.alpha, sim.config.alpha);
        assert_eq!(sim.iteration, 0);
        assert_eq!(sim.config.link_distance, 80.0);
        assert_eq!(sim.config.charge_strength, -200.0);
        assert_eq!(sim.config.center, (5.0, 6.0));
    }

    #[test]
    fn initialize_reseeds_every_node_by_default() {
        let mut graph = VowlGraph::new();
        graph
            .add_node(NodeBuilder::new("placed").position(7.0, 9.0).build())
            .unwrap();

        let mut sim = ForceSimulation::new();
        sim.initialize(&mut graph).unwrap();

        let node = graph.get_node("placed").unwrap();
        assert_ne!((node.visual.x, node.visual.y), (7.0, 9.0));
        let r = node.visual.x.hypot(node.visual.y);
        assert!(
            (r - LayoutConfig::default().seed_radius).abs() < 1e-6,
            "seeded onto the circle: {r}"
        );
    }

    #[test]
    fn initialize_keeps_placed_nodes_when_preserving() {
        let mut graph = VowlGraph::new();
        graph
            .add_node(NodeBuilder::new("placed").position(7.0, 9.0).build())
            .unwrap();
        graph
            .add_node(NodeBuilder::new("at_origin").position(0.0, 0.0).build())
            .unwrap();

        let config = LayoutConfig {
            preserve_seeded_positions: true,
            ..LayoutConfig::default()
        };
        let mut sim = ForceSimulation::with_config(config);
        sim.initialize(&mut graph).unwrap();

        // The deliberately placed node survives untouched...
        let placed = graph.get_node("placed").unwrap();
        assert_eq!((placed.visual.x, placed.visual.y), (7.0, 9.0));
        // ...while the one still at the origin is seeded onto the circle.
        let seeded = graph.get_node("at_origin").unwrap();
        assert_ne!((seeded.visual.x, seeded.visual.y), (0.0, 0.0));
        // Velocities are reset either way.
        assert_eq!((placed.visual.vx, placed.visual.vy), (0.0, 0.0));
    }
}
