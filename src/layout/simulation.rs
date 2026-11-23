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
    /// Per-node collision radii in node index order, or empty to use
    /// `config.collision_radius` for every node.
    node_radii: Vec<f64>,
}

impl ForceSimulation {
    /// Create a new simulation with default configuration
    pub fn new() -> Self {
        Self {
            config: LayoutConfig::default(),
            alpha: 1.0,
            iteration: 0,
            debug_flags: DebugFlags::default(),
            node_radii: Vec::new(),
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
            node_radii: Vec::new(),
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

    /// The configuration in force.
    pub fn config(&self) -> &LayoutConfig {
        &self.config
    }

    /// Give every node the collision radius `radius`
    /// ([`LayoutConfig::collision_radius`]); `0.0` turns collision off.
    /// A negative or non-finite value is ignored.
    pub fn set_collision_radius(&mut self, radius: f64) {
        if radius.is_finite() && radius >= 0.0 {
            self.config.collision_radius = radius;
        }
    }

    /// Set [`LayoutConfig::collision_strength`], clamped to `[0, 1]`. A
    /// non-finite value is ignored.
    pub fn set_collision_strength(&mut self, strength: f64) {
        if strength.is_finite() {
            self.config.collision_strength = strength.clamp(0.0, 1.0);
        }
    }

    /// Set [`LayoutConfig::collision_iterations`]. Zero is ignored; turn
    /// collision off with a zero radius instead.
    pub fn set_collision_iterations(&mut self, iterations: usize) {
        if iterations > 0 {
            self.config.collision_iterations = iterations;
        }
    }

    /// Set [`LayoutConfig::gravity_strength`], clamped to `[0, 1]`; `0.0`
    /// turns it off. A negative or non-finite value is ignored.
    pub fn set_gravity_strength(&mut self, strength: f64) {
        if strength.is_finite() && strength >= 0.0 {
            self.config.gravity_strength = strength.min(1.0);
        }
    }

    /// Set [`LayoutConfig::alpha_decay`], the fraction of alpha lost per
    /// tick. A value outside `(0, 1)` or non-finite is ignored.
    ///
    /// The run lasts about `ln(alpha_min) / ln(1 - alpha_decay)` ticks: 300
    /// at the default `0.0228` and `0.001`, about 690 at `0.01`.
    pub fn set_alpha_decay(&mut self, decay: f64) {
        if decay.is_finite() && decay > 0.0 && decay < 1.0 {
            self.config.alpha_decay = decay;
        }
    }

    /// Set [`LayoutConfig::alpha_min`], the alpha below which the simulation
    /// counts as finished. A value outside `(0, 1)` or non-finite is ignored.
    pub fn set_alpha_min(&mut self, alpha_min: f64) {
        if alpha_min.is_finite() && alpha_min > 0.0 && alpha_min < 1.0 {
            self.config.alpha_min = alpha_min;
        }
    }

    /// Set [`LayoutConfig::velocity_decay`], the fraction of velocity
    /// **kept** per tick (`0.6` is d3-force's `velocityDecay(0.4)`). A value
    /// outside `[0, 1]` or non-finite is ignored.
    pub fn set_velocity_decay(&mut self, retained: f64) {
        if retained.is_finite() && (0.0..=1.0).contains(&retained) {
            self.config.velocity_decay = retained;
        }
    }

    /// Give each node its own collision radius, in node index order, for a
    /// graph of `node_count` nodes. An empty slice clears them, returning
    /// every node to [`LayoutConfig::collision_radius`].
    ///
    /// The radii belong to one graph: a caller loading another graph should
    /// clear them (the `WebVowl` binding does so in `loadOntology`).
    ///
    /// # Errors
    ///
    /// The length is not `node_count`, or a radius is negative or not finite.
    /// The previous radii are kept.
    pub fn set_node_radii(
        &mut self,
        radii: &[f64],
        node_count: usize,
    ) -> std::result::Result<(), String> {
        if radii.is_empty() {
            self.node_radii.clear();
            return Ok(());
        }
        if radii.len() != node_count {
            return Err(format!(
                "expected {node_count} radii, one per node, got {}",
                radii.len()
            ));
        }
        if let Some(i) = radii.iter().position(|r| !(r.is_finite() && *r >= 0.0)) {
            return Err(format!(
                "radius {i} is {}; radii must be finite and non-negative",
                radii[i]
            ));
        }
        self.node_radii = radii.to_vec();
        Ok(())
    }

    /// Drop the per-node radii set by [`set_node_radii`](Self::set_node_radii).
    pub fn clear_node_radii(&mut self) {
        self.node_radii.clear();
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

        // Positional gravity (d3-force forceX / forceY towards the centre).
        // Skipped entirely when off, so the default arithmetic is unchanged.
        let gravity = self.config.gravity_strength;
        if gravity > 0.0 {
            let (cx, cy) = self.config.center;
            for (force, &(x, y)) in forces.iter_mut().zip(&positions) {
                force.x += (cx - x) * gravity;
                force.y += (cy - y) * gravity;
            }
        }

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

    /// Integrate one step: `v = (v + alpha * F) * velocity_decay`, `x += v`,
    /// with collision applied to `v + alpha * F` before the decay, as
    /// d3-force orders it.
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

        let mut velocities: Vec<Vector2<f64>> = graph
            .nodes()
            .iter()
            .zip(forces)
            .map(|(node, force)| {
                if node.visual.fixed {
                    Vector2::zeros()
                } else {
                    Vector2::new(node.visual.vx, node.visual.vy) + force * self.alpha
                }
            })
            .collect();

        if self.collision_active(graph.node_count()) {
            self.collide(graph, &mut velocities);
        }

        for (node, velocity) in graph.nodes_mut().zip(velocities) {
            if node.visual.fixed {
                continue;
            }
            let mut velocity = velocity * self.config.velocity_decay;
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

    /// Collision radius of node `index`.
    fn radius(&self, index: usize) -> f64 {
        self.node_radii
            .get(index)
            .copied()
            .unwrap_or(self.config.collision_radius)
    }

    fn collision_active(&self, node_count: usize) -> bool {
        let any_radius = if self.node_radii.len() == node_count {
            self.node_radii.iter().any(|&r| r > 0.0)
        } else {
            self.config.collision_radius > 0.0
        };
        any_radius && self.config.collision_strength > 0.0 && self.config.collision_iterations > 0
    }

    /// d3-force's `forceCollide`, on the velocities of this tick.
    ///
    /// Each pass predicts every node at `x + v`, finds the pairs closer than
    /// `r_i + r_j` and moves them apart by `strength` of the overlap, the
    /// node with the smaller radius moving more (`r_j² / (r_i² + r_j²)` of
    /// the correction to node `i`). A fixed node does not move, so the other
    /// node takes the whole correction. Velocities are updated in place in
    /// node index order, so later pairs see earlier corrections, as in d3.
    ///
    /// Candidate pairs come from a uniform grid with cells as wide as the
    /// largest possible contact distance, so a pass costs `O(n)` for a
    /// layout of bounded density instead of `O(n²)`.
    fn collide(&self, graph: &VowlGraph, velocities: &mut [Vector2<f64>]) {
        use std::collections::HashMap;

        let nodes = graph.nodes();
        let n = nodes.len();
        let radii: Vec<f64> = (0..n).map(|i| self.radius(i)).collect();
        let fixed: Vec<bool> = nodes.iter().map(|node| node.visual.fixed).collect();
        let origin: Vec<Vector2<f64>> = nodes
            .iter()
            .map(|node| Vector2::new(node.visual.x, node.visual.y))
            .collect();
        let max_radius = radii.iter().copied().fold(0.0, f64::max);
        let cell = 2.0 * max_radius;
        if !(cell.is_finite() && cell > 0.0) {
            return;
        }
        let strength = self.config.collision_strength;
        let cell_of = |p: Vector2<f64>| ((p.x / cell).floor() as i64, (p.y / cell).floor() as i64);

        for _ in 0..self.config.collision_iterations {
            let predicted: Vec<Vector2<f64>> = (0..n).map(|i| origin[i] + velocities[i]).collect();
            if predicted
                .iter()
                .any(|p| !(p.x.is_finite() && p.y.is_finite()))
            {
                return;
            }
            let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
            for (i, &p) in predicted.iter().enumerate() {
                grid.entry(cell_of(p)).or_default().push(i);
            }

            for i in 0..n {
                let ri = radii[i];
                // Node i's predicted position is fixed for its pass, as in d3.
                let pi = predicted[i];
                let (gx, gy) = cell_of(pi);
                for key in (gx - 1..=gx + 1).flat_map(|x| (gy - 1..=gy + 1).map(move |y| (x, y))) {
                    let Some(members) = grid.get(&key) else {
                        continue;
                    };
                    for &j in members {
                        if j <= i || (fixed[i] && fixed[j]) {
                            continue;
                        }
                        let rj = radii[j];
                        let r = ri + rj;
                        let pj = origin[j] + velocities[j];
                        let mut x = pi.x - pj.x;
                        let mut y = pi.y - pj.y;
                        let mut l = x * x + y * y;
                        if l >= r * r {
                            continue;
                        }
                        // Coincident on an axis: a deterministic nudge stands
                        // in for d3's random jiggle.
                        if x == 0.0 {
                            x = jiggle(i, j);
                            l += x * x;
                        }
                        if y == 0.0 {
                            y = jiggle(j, i);
                            l += y * y;
                        }
                        let distance = l.sqrt();
                        let scale = (r - distance) / distance * strength;
                        x *= scale;
                        y *= scale;
                        let (ri2, rj2) = (ri * ri, rj * rj);
                        let share_i = if fixed[j] {
                            1.0
                        } else if fixed[i] {
                            0.0
                        } else {
                            rj2 / (ri2 + rj2)
                        };
                        velocities[i].x += x * share_i;
                        velocities[i].y += y * share_i;
                        velocities[j].x -= x * (1.0 - share_i);
                        velocities[j].y -= y * (1.0 - share_i);
                    }
                }
            }
        }
    }
}

/// A tiny, deterministic, non-zero offset for a coincident pair: d3-force's
/// `jiggle` without the random source, so layouts stay reproducible.
fn jiggle(a: usize, b: usize) -> f64 {
    let sign = if (a ^ b) & 1 == 0 { 1.0 } else { -1.0 };
    sign * 1e-6 * (1.0 + ((a.wrapping_mul(31) + b) % 7) as f64)
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

    #[test]
    fn set_node_radii_validates_and_clears() {
        let mut sim = ForceSimulation::new();
        assert!(sim.set_node_radii(&[1.0, 2.0], 3).is_err());
        assert!(sim.set_node_radii(&[1.0, f64::NAN, 2.0], 3).is_err());
        assert!(sim.set_node_radii(&[1.0, -2.0, 2.0], 3).is_err());
        assert!(
            sim.node_radii.is_empty(),
            "a rejected call keeps the old radii"
        );
        sim.set_node_radii(&[1.0, 2.0, 3.0], 3).unwrap();
        assert_eq!(sim.radius(2), 3.0);
        sim.set_node_radii(&[], 3).unwrap();
        sim.set_collision_radius(7.0);
        assert_eq!(sim.radius(2), 7.0);
    }

    #[test]
    fn collision_separates_coincident_nodes() {
        let mut graph = VowlGraph::new();
        graph
            .add_node(class("a").position(5.0, 5.0).build())
            .unwrap();
        graph
            .add_node(class("b").position(5.0, 5.0).build())
            .unwrap();
        let mut sim = ForceSimulation::with_config(LayoutConfig {
            preserve_seeded_positions: true,
            charge_strength: 0.0,
            collision_radius: 10.0,
            ..LayoutConfig::default()
        });
        settle(&mut graph, &mut sim);
        let d = distance(&graph, "a", "b");
        assert!(d >= 19.0, "coincident pair only {d} apart");
    }

    #[test]
    fn collision_does_not_move_a_pinned_node() {
        let mut graph = VowlGraph::new();
        graph
            .add_node(class("pinned").position(0.0, 0.0).build())
            .unwrap();
        graph
            .add_node(class("free").position(1.0, 0.0).build())
            .unwrap();
        graph.get_node_mut("pinned").unwrap().visual.fixed = true;
        let mut sim = ForceSimulation::with_config(LayoutConfig {
            preserve_seeded_positions: true,
            charge_strength: 0.0,
            center_strength: 0.0,
            collision_radius: 10.0,
            ..LayoutConfig::default()
        });
        sim.initialize(&mut graph).unwrap();
        // `initialize` reseeds a node at the origin; put it back.
        let pinned = graph.get_node_mut("pinned").unwrap();
        (pinned.visual.x, pinned.visual.y) = (0.0, 0.0);
        while !sim.is_finished() {
            sim.tick(&mut graph).unwrap();
        }
        let pinned = graph.get_node("pinned").unwrap();
        assert_eq!((pinned.visual.x, pinned.visual.y), (0.0, 0.0));
        assert!(distance(&graph, "pinned", "free") >= 19.0);
    }

    #[test]
    fn gravity_pulls_an_unlinked_pair_towards_the_centre() {
        // Two unlinked nodes: charge alone pushes them apart without limit
        // (the centroid correction holds only their midpoint); gravity bounds
        // the spread.
        let spread = |gravity: f64| {
            let mut graph = create_test_graph();
            let mut sim = ForceSimulation::with_config(LayoutConfig {
                charge_strength: -300.0,
                gravity_strength: gravity,
                ..LayoutConfig::default()
            });
            settle(&mut graph, &mut sim);
            graph
                .nodes()
                .iter()
                .map(|n| n.visual.x.hypot(n.visual.y))
                .fold(0.0, f64::max)
        };
        let (free, held) = (spread(0.0), spread(0.1));
        assert!(held < 0.5 * free, "gravity 0.1: {held:.1}, off: {free:.1}");
    }

    #[test]
    fn schedule_setters_validate() {
        let mut sim = ForceSimulation::new();
        let before = sim.config.clone();
        sim.set_alpha_decay(0.0);
        sim.set_alpha_decay(1.0);
        sim.set_alpha_min(0.0);
        sim.set_velocity_decay(1.5);
        sim.set_collision_iterations(0);
        sim.set_gravity_strength(-1.0);
        assert_eq!(sim.config.alpha_decay, before.alpha_decay);
        assert_eq!(sim.config.alpha_min, before.alpha_min);
        assert_eq!(sim.config.velocity_decay, before.velocity_decay);
        assert_eq!(sim.config.collision_iterations, 1);
        assert_eq!(sim.config.gravity_strength, 0.0);
        sim.set_alpha_decay(0.01);
        sim.set_alpha_min(0.0001);
        sim.set_velocity_decay(0.4);
        sim.set_collision_strength(3.0);
        sim.set_gravity_strength(2.0);
        assert_eq!(sim.config.alpha_decay, 0.01);
        assert_eq!(sim.config.alpha_min, 0.0001);
        assert_eq!(sim.config.velocity_decay, 0.4);
        assert_eq!(sim.config.collision_strength, 1.0);
        assert_eq!(sim.config.gravity_strength, 1.0);
    }
}
