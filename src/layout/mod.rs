//! Force-directed layout algorithms
//!
//! This module implements physics-based graph layout algorithms,
//! primarily force-directed layouts similar to D3.js force simulations.

pub mod csr_sim;
pub mod force;
pub mod quadtree;
pub mod simd;
pub mod simulation;

use crate::graph::VowlGraph;
use crate::Result;

/// Trait for layout algorithms
#[cfg_attr(test, mockall::automock)]
pub trait LayoutAlgorithm {
    /// Initialize the layout
    fn initialize(&mut self, graph: &mut VowlGraph) -> Result<()>;

    /// Perform one tick of the simulation
    fn tick(&mut self, graph: &mut VowlGraph) -> Result<()>;

    /// Run the simulation for n iterations
    fn run(&mut self, graph: &mut VowlGraph, iterations: usize) -> Result<()>;

    /// Check if simulation is finished
    fn is_finished(&self) -> bool;

    /// Get current alpha (simulation progress)
    fn alpha(&self) -> f64;
}

/// Configuration for force-directed layout
#[derive(Debug, Clone)]
pub struct LayoutConfig {
    /// Simulation alpha (energy)
    pub alpha: f64,

    /// Alpha decay rate
    pub alpha_decay: f64,

    /// Minimum alpha before stopping
    pub alpha_min: f64,

    /// Velocity decay (damping)
    pub velocity_decay: f64,

    /// Link distance
    pub link_distance: f64,

    /// Link strength
    pub link_strength: f64,

    /// Charge strength (repulsion)
    pub charge_strength: f64,

    /// Center force strength
    pub center_strength: f64,

    /// Center position
    pub center: (f64, f64),

    /// Barnes-Hut theta parameter (0.0-1.0)
    /// Lower = more accurate but slower
    /// Higher = faster but less accurate
    /// Default: 0.9 (good balance)
    pub barnes_hut_theta: f64,

    /// Enable Barnes-Hut optimization
    /// When enabled, uses O(n log n) force approximation
    /// When disabled, uses exact O(n²) calculation
    pub use_barnes_hut: bool,

    /// Radius of the circle nodes are seeded onto by `initialize`.
    ///
    /// A larger radius gives the repulsion force more room to work before the
    /// layout starts contracting, which matters for dense graphs.
    pub seed_radius: f64,

    /// Keep node positions the caller already set, instead of reseeding them.
    ///
    /// `false` (the default) reseeds every node onto the circle on `initialize`,
    /// giving a deterministic starting state regardless of what the graph
    /// arrived holding. Set it to `true` to resume a saved layout, or to honour
    /// positions a caller placed deliberately: nodes still at the origin are
    /// seeded as usual, and every other node is left where it is.
    pub preserve_seeded_positions: bool,
}

impl Default for LayoutConfig {
    /// General-purpose defaults, suitable for graphs of any size.
    ///
    /// The force constants are the conventional force-directed values (a 30-unit
    /// rest length, unit link strength, `-30` charge and full centre pull), which
    /// behave sensibly from a handful of nodes upward. For graphs in the
    /// thousands these settle into a dense hairball; use
    /// [`LayoutConfig::large_graph`] there instead.
    fn default() -> Self {
        Self {
            alpha: 1.0,
            alpha_decay: 0.0228,
            alpha_min: 0.001,
            velocity_decay: 0.6,
            link_distance: 30.0,
            link_strength: 1.0,
            charge_strength: -30.0,
            center_strength: 1.0,
            center: (0.0, 0.0),
            barnes_hut_theta: 0.9,
            use_barnes_hut: true,
            seed_radius: 300.0,
            preserve_seeded_positions: false,
        }
    }
}

impl LayoutConfig {
    /// Force constants tuned for a large, densely connected ontology.
    ///
    /// These are the values the DreamLab corpus explorer runs at, arrived at
    /// empirically on a 1,155-node ontology: a long rest length and a strong
    /// charge to force the graph open, weak links so it can expand, and almost
    /// no centre pull so clusters are not crushed together. They deliberately
    /// over-spread a small graph, which is why they are a named preset rather
    /// than the default.
    ///
    /// ```
    /// use vowl_wasm::layout::LayoutConfig;
    ///
    /// let big = LayoutConfig::large_graph();
    /// let small = LayoutConfig::default();
    ///
    /// assert!(big.charge_strength < small.charge_strength, "stronger repulsion");
    /// assert!(big.link_distance > small.link_distance, "longer rest length");
    /// assert!(big.use_barnes_hut, "O(n log n) forces matter at this size");
    /// ```
    pub fn large_graph() -> Self {
        Self {
            link_distance: 250.0,
            link_strength: 0.5,
            charge_strength: -2000.0,
            center_strength: 0.001,
            seed_radius: 2000.0,
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = LayoutConfig::default();
        assert_eq!(config.alpha, 1.0);
        assert!(config.alpha_decay > 0.0);
        assert!(config.alpha_min > 0.0);
    }

    #[test]
    fn test_config_values() {
        let config = LayoutConfig {
            alpha: 0.5,
            link_distance: 50.0,
            ..Default::default()
        };

        assert_eq!(config.alpha, 0.5);
        assert_eq!(config.link_distance, 50.0);
    }
}
