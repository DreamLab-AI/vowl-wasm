//! `CsrSimulation` — deterministic force-directed layout over an NGG1 tier.
//!
//! This is the physics half of ADR-NG-001 §3: the worker owns one of these,
//! seeds it from the NGG1 node positions (warm start), and ticks it off the main
//! thread. Positions live in a single interleaved `Vec<f32>` (`[x0,y0,x1,y1,…]`)
//! whose pointer is handed to JS as a zero-copy `Float32Array` view — the
//! VisionClaw `get_*_ptr` idiom (`scene-effects`), adapted.
//!
//! Design constraints (ADR §3):
//! - Many-body repulsion is Barnes-Hut by default: a 2D quadtree is built once
//!   per tick and distant cells are collapsed to their total charge at their
//!   charge-weighted centre when `cell_width² / distance² < theta²` (`theta`
//!   defaults to 0.9, the d3-force `forceManyBody` default). The pair formula
//!   (`charge / d²`, index-derived nudge for coincident nodes) is the same as
//!   the exact loop. Select the exact O(n²) all-pairs loop with
//!   [`Repulsion::Exact`], `set_theta(0.0)` or `set_param("theta", 0.0)`; it
//!   runs unchanged, so results can be compared against the approximation.
//! - Springs on both edge kinds (subClassOf backbone + objectProperty relation).
//! - **Deterministic** given the seeds: node order is the CSR local-index order,
//!   all accumulation is over `Vec` indices (no `HashMap` iteration), coincident
//!   nodes get an index-derived perturbation — so two runs from the same tier
//!   produce identical buffers.
//! - Auto-finish at the alpha floor (`is_finished`), matching `ngg1.ts`/protocol
//!   `Settled`.

#[cfg(feature = "ngg1")]
use crate::ngg1::Ngg1;

/// Many-body repulsion strategy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Repulsion {
    /// Exact O(n²) all-pairs loop.
    Exact,
    /// Barnes-Hut quadtree approximation with the given opening angle `theta`
    /// (larger ⇒ faster, less accurate). A `theta` of zero or below is
    /// normalised to [`Repulsion::Exact`] by [`Repulsion::from_theta`].
    BarnesHut(f32),
}

impl Repulsion {
    /// Map a d3-style `theta` to a strategy: `theta <= 0` (or non-finite) is
    /// [`Repulsion::Exact`], anything else is [`Repulsion::BarnesHut`].
    pub fn from_theta(theta: f32) -> Self {
        if theta.is_finite() && theta > 0.0 {
            Repulsion::BarnesHut(theta)
        } else {
            Repulsion::Exact
        }
    }
}

/// Tunable scalar parameters. These are the only physics knobs exposed to JS
/// (protocol `SimParamKey`); no WebVOWL-style settings sprawl.
#[derive(Debug, Clone, Copy)]
pub struct SimConfig {
    /// Repulsion constant (Coulomb). **Positive = repulsive** (nodes push
    /// apart). Larger ⇒ more spread.
    pub charge: f32,
    /// Spring rest length for edges.
    pub link_distance: f32,
    /// Spring stiffness (Hooke). 0 ⇒ edges exert no pull.
    pub link_strength: f32,
    /// Per-tick velocity retention (damping). `v *= damping`. Lower ⇒ settles
    /// faster.
    pub damping: f32,
    /// Pull toward `center` each tick, scaled by this.
    pub center_gravity: f32,
    /// Layout centre.
    pub center: (f32, f32),
    /// Repulsion strategy (default Barnes-Hut, `theta = 0.9`).
    pub repulsion: Repulsion,
    /// Starting annealing temperature.
    pub alpha: f32,
    /// Per-tick multiplicative alpha decay: `alpha *= 1 - alpha_decay`.
    pub alpha_decay: f32,
    /// Alpha floor — below this the sim is `is_finished()` and ticks are no-ops.
    pub alpha_min: f32,
}

impl Default for SimConfig {
    fn default() -> Self {
        // Tuned for a scoped tier (≤1,500 nodes) on a cream editorial canvas:
        // strong spread, weak centre pull, a rest length that reads as airy.
        Self {
            charge: 2000.0,
            link_distance: 60.0,
            link_strength: 0.5,
            damping: 0.6,
            center_gravity: 0.02,
            center: (0.0, 0.0),
            repulsion: Repulsion::BarnesHut(0.9),
            alpha: 1.0,
            alpha_decay: 0.0228,
            alpha_min: 0.001,
        }
    }
}

const NONE: u32 = u32::MAX;

/// Repulsion softening radius as a fraction of `link_distance`. `charge / d²`
/// is unbounded as `d → 0`, so a near-coincident seed pair would be thrown
/// thousands of units in one tick; below this radius the magnitude is held at
/// its value at the radius (direction still follows the true offset).
const SOFTENING_FRACTION: f32 = 0.25;
/// Per-tick displacement cap as a multiple of `link_distance`.
const MAX_STEP_FACTOR: f32 = 2.0;
/// Depth cap: coincident points end up in one multi-point leaf instead of
/// recursing forever.
const MAX_DEPTH: u32 = 40;

/// Pair displacement `(dx, dy, d²)` for the pair `(lo, hi)` (`lo < hi`),
/// from the point of view of the node on the `i` side (`is_lo` ⇒ `i == lo`).
/// Mirrors the exact loop, including the deterministic coincident nudge.
#[inline]
fn pair_delta(dx: f32, dy: f32, lo: usize, hi: usize, is_lo: bool) -> (f32, f32, f32) {
    let d2 = dx * dx + dy * dy;
    if d2 >= 1e-6 {
        return (dx, dy, d2);
    }
    let s = (lo as f32) * 12.9898 + (hi as f32) * 78.233;
    let (mut ndx, mut ndy) = (s.sin() * 1e-2, s.cos() * 1e-2);
    let mut nd2 = ndx * ndx + ndy * ndy;
    if nd2 < 1e-9 {
        ndx = 1e-2;
        ndy = 0.0;
        nd2 = 1e-4;
    }
    if is_lo {
        (ndx, ndy, nd2)
    } else {
        (-ndx, -ndy, nd2)
    }
}

/// One quadtree cell. Leaves own `points[start..start + count]` (`start` is
/// meaningless for internal cells); every cell carries charge-weighted sums
/// over all its descendants.
struct Cell {
    cx: f32,
    cy: f32,
    half: f32,
    /// Total charge (unit charge per node ⇒ node count).
    count: u32,
    /// Sum of member coordinates; `sx / count` is the centre of charge.
    sx: f32,
    sy: f32,
    start: u32,
    children: [u32; 4],
}

impl Cell {
    #[inline]
    fn is_leaf(&self) -> bool {
        self.children == [NONE; 4]
    }
}

/// Arena quadtree over the interleaved position buffer, rebuilt every tick.
struct QuadTree {
    cells: Vec<Cell>,
    /// Node indices, grouped so each cell's members are contiguous.
    points: Vec<u32>,
}

impl QuadTree {
    /// Build over `n` points; `None` if there are none or any coordinate is
    /// non-finite.
    fn build(pos: &[f32], n: usize) -> Option<Self> {
        if n == 0 {
            return None;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for i in 0..n {
            let (x, y) = (pos[i * 2], pos[i * 2 + 1]);
            if !x.is_finite() || !y.is_finite() {
                return None;
            }
            x0 = x0.min(x);
            x1 = x1.max(x);
            y0 = y0.min(y);
            y1 = y1.max(y);
        }
        let half = ((x1 - x0).max(y1 - y0) * 0.5).max(1e-3) * 1.0001;
        let mut tree = QuadTree {
            cells: Vec::with_capacity(n * 2),
            points: Vec::with_capacity(n),
        };
        let idx: Vec<u32> = (0..n as u32).collect();
        tree.add(pos, idx, (x0 + x1) * 0.5, (y0 + y1) * 0.5, half, 0);
        Some(tree)
    }

    /// Recursively build a cell for `idx`, returning its arena index.
    fn add(&mut self, pos: &[f32], idx: Vec<u32>, cx: f32, cy: f32, half: f32, depth: u32) -> u32 {
        let me = self.cells.len() as u32;
        let (mut sx, mut sy) = (0.0f32, 0.0f32);
        for &i in &idx {
            sx += pos[i as usize * 2];
            sy += pos[i as usize * 2 + 1];
        }
        let start = self.points.len() as u32;
        self.cells.push(Cell {
            cx,
            cy,
            half,
            count: idx.len() as u32,
            sx,
            sy,
            start,
            children: [NONE; 4],
        });
        if idx.len() <= 1 || depth >= MAX_DEPTH {
            self.points.extend_from_slice(&idx);
            return me;
        }
        let mut quads: [Vec<u32>; 4] = Default::default();
        for &i in &idx {
            let q = usize::from(pos[i as usize * 2] >= cx)
                | (usize::from(pos[i as usize * 2 + 1] >= cy) << 1);
            quads[q].push(i);
        }
        let h = half * 0.5;
        for (q, members) in quads.into_iter().enumerate() {
            if members.is_empty() {
                continue;
            }
            let qx = cx + if q & 1 == 1 { h } else { -h };
            let qy = cy + if q & 2 == 2 { h } else { -h };
            let child = self.add(pos, members, qx, qy, h, depth + 1);
            self.cells[me as usize].children[q] = child;
        }
        me
    }
}

/// A live simulation. Owns the interleaved position buffer and the CSR-derived
/// spring list; nothing crosses the WASM boundary but the position pointer.
pub struct CsrSimulation {
    config: SimConfig,
    n: usize,
    /// Interleaved live positions `[x0,y0,…]`, length `n*2`. Stable address
    /// after `load_csr` (no reallocation during `tick`) — this is what
    /// `positions_ptr` exposes.
    positions: Vec<f32>,
    /// Interleaved velocities, length `n*2`.
    velocities: Vec<f32>,
    /// Nodes held in place by the host (a pointer drag), length `n`. A pinned
    /// node still repels and pulls on the others; it is only skipped when
    /// positions are integrated. Cleared by every load.
    pinned: Vec<bool>,
    /// Edge endpoints (local indices) and types, parallel arrays.
    edge_a: Vec<u32>,
    edge_b: Vec<u32>,
    edge_type: Vec<u8>,
    alpha: f32,
    finished: bool,
}

impl Default for CsrSimulation {
    fn default() -> Self {
        Self::new()
    }
}

impl CsrSimulation {
    /// A fresh, empty simulation with default parameters. Load a tier before
    /// ticking.
    pub fn new() -> Self {
        let config = SimConfig::default();
        Self {
            n: 0,
            positions: Vec::new(),
            velocities: Vec::new(),
            pinned: Vec::new(),
            edge_a: Vec::new(),
            edge_b: Vec::new(),
            edge_type: Vec::new(),
            alpha: config.alpha,
            config,
            finished: false,
        }
    }

    /// Build simulation state from plain arrays, without the NGG1 format.
    ///
    /// This is the format-independent loader: `positions` is the interleaved
    /// warm start `[x0, y0, x1, y1, …]` (so `positions.len() == n * 2`), and the
    /// three edge arrays are parallel — `edge_a[e]`/`edge_b[e]` are node indices
    /// and `edge_type[e]` classifies the spring. Velocities are zeroed and the
    /// annealing temperature is reset, exactly as [`load_csr`] does.
    ///
    /// Edges naming an out-of-range node, and self-edges, are skipped at tick
    /// time rather than rejected here.
    ///
    /// # Panics
    ///
    /// Panics if `positions.len()` is odd, or if the three edge arrays are not
    /// all the same length.
    ///
    /// ```
    /// use vowl_wasm::layout::csr_sim::CsrSimulation;
    ///
    /// let mut sim = CsrSimulation::new();
    /// sim.load_arrays(vec![0.0, 0.0, 10.0, 0.0], vec![0], vec![1], vec![0]);
    ///
    /// assert_eq!(sim.node_count(), 2);
    /// assert_eq!(sim.edge_count(), 1);
    /// ```
    ///
    /// [`load_csr`]: CsrSimulation::load_csr
    pub fn load_arrays(
        &mut self,
        positions: Vec<f32>,
        edge_a: Vec<u32>,
        edge_b: Vec<u32>,
        edge_type: Vec<u8>,
    ) {
        assert!(
            positions.len() % 2 == 0,
            "positions must be interleaved [x,y,…]; got an odd length of {}",
            positions.len()
        );
        assert!(
            edge_a.len() == edge_b.len() && edge_b.len() == edge_type.len(),
            "edge arrays must be parallel; got {}/{}/{}",
            edge_a.len(),
            edge_b.len(),
            edge_type.len()
        );

        self.n = positions.len() / 2;
        self.velocities = vec![0.0f32; positions.len()];
        self.pinned = vec![false; positions.len() / 2];
        self.positions = positions;
        self.edge_a = edge_a;
        self.edge_b = edge_b;
        self.edge_type = edge_type;
        self.alpha = self.config.alpha;
        self.finished = self.n == 0;
    }

    /// Parse an NGG1 buffer and build simulation state from it: seed positions
    /// from the node warm-start `x,y`, build springs from the CSR adjacency
    /// (both edge kinds), reset velocities and the annealing temperature.
    ///
    /// Zero-copy at the parse layer (fields are decoded on demand from the
    /// borrowed bytes); the owned working arrays are built once here. Idempotent:
    /// a second `load_csr` swaps the whole scope.
    ///
    /// # Errors
    ///
    /// Returns [`Ngg1Error`](crate::ngg1::Ngg1Error) if the buffer is not a
    /// well-formed NGG1 tier — a bad magic number, an unsupported version, or a
    /// truncated body.
    #[cfg(feature = "ngg1")]
    #[cfg_attr(docsrs, doc(cfg(feature = "ngg1")))]
    pub fn load_csr(&mut self, bytes: &[u8]) -> Result<(), crate::ngg1::Ngg1Error> {
        let g = Ngg1::parse(bytes)?;
        self.load(&g);
        Ok(())
    }

    /// Build simulation state from an already-parsed tier.
    #[cfg(feature = "ngg1")]
    #[cfg_attr(docsrs, doc(cfg(feature = "ngg1")))]
    pub fn load(&mut self, g: &Ngg1<'_>) {
        self.n = g.node_count();
        self.positions = g.seed_positions();
        self.velocities = vec![0.0f32; self.n * 2];
        self.pinned = vec![false; self.n];
        let (a, b, ty) = g.edges();
        self.edge_a = a;
        self.edge_b = b;
        self.edge_type = ty;
        self.alpha = self.config.alpha;
        self.finished = self.n == 0;
    }

    /// Advance the simulation one step. No-op once finished. Deterministic.
    pub fn tick(&mut self) {
        if self.finished || self.n == 0 {
            return;
        }
        let n = self.n;
        let mut fx = vec![0.0f32; n];
        let mut fy = vec![0.0f32; n];

        // ── Repulsion: exact all-pairs or Barnes-Hut (see module doc).
        self.repulsion_forces(&mut fx, &mut fy);

        // ── Springs: Hooke toward rest length, both backbone and relation.
        let ld = self.config.link_distance;
        let ls = self.config.link_strength;
        for e in 0..self.edge_a.len() {
            let a = self.edge_a[e] as usize;
            let b = self.edge_b[e] as usize;
            if a >= n || b >= n || a == b {
                continue;
            }
            let dx = self.positions[b * 2] - self.positions[a * 2];
            let dy = self.positions[b * 2 + 1] - self.positions[a * 2 + 1];
            let dist = (dx * dx + dy * dy).sqrt().max(1e-4);
            let disp = dist - ld;
            let f = disp * ls / dist;
            let fxe = dx * f;
            let fye = dy * f;
            fx[a] += fxe;
            fy[a] += fye;
            fx[b] -= fxe;
            fy[b] -= fye;
        }

        // ── Centre gravity + symplectic-Euler integration with damping.
        let (cx, cy) = self.config.center;
        let cg = self.config.center_gravity;
        let damping = self.config.damping;
        let alpha = self.alpha;
        let max_step = MAX_STEP_FACTOR * self.config.link_distance;
        for i in 0..n {
            if self.pinned[i] {
                continue;
            }
            let px = self.positions[i * 2];
            let py = self.positions[i * 2 + 1];
            let ax = fx[i] + (cx - px) * cg;
            let ay = fy[i] + (cy - py) * cg;

            let mut vx = self.velocities[i * 2] + ax * alpha;
            let mut vy = self.velocities[i * 2 + 1] + ay * alpha;
            vx *= damping;
            vy *= damping;
            let speed = (vx * vx + vy * vy).sqrt();
            if speed > max_step {
                let k = max_step / speed;
                vx *= k;
                vy *= k;
            }
            self.velocities[i * 2] = vx;
            self.velocities[i * 2 + 1] = vy;
            self.positions[i * 2] = px + vx;
            self.positions[i * 2 + 1] = py + vy;
        }

        self.alpha *= 1.0 - self.config.alpha_decay;
        if self.alpha < self.config.alpha_min {
            self.alpha = self.config.alpha_min;
            self.finished = true;
        }
    }

    /// Squared softening radius for the repulsion magnitude.
    #[inline]
    fn softening_sq(&self) -> f32 {
        let r = SOFTENING_FRACTION * self.config.link_distance;
        r * r
    }

    /// Accumulate many-body repulsion into `fx`/`fy` (length `n`) using the
    /// configured strategy.
    fn repulsion_forces(&self, fx: &mut [f32], fy: &mut [f32]) {
        match self.config.repulsion {
            Repulsion::Exact => self.repulsion_exact(fx, fy),
            Repulsion::BarnesHut(theta) => self.repulsion_barnes_hut(theta, fx, fy),
        }
    }

    /// O(n²) Coulomb, every unordered pair once (Newton's 3rd).
    fn repulsion_exact(&self, fx: &mut [f32], fy: &mut [f32]) {
        let n = self.n;
        let charge = self.config.charge;
        let soft2 = self.softening_sq();
        for i in 0..n {
            let xi = self.positions[i * 2];
            let yi = self.positions[i * 2 + 1];
            for j in (i + 1)..n {
                let mut dx = xi - self.positions[j * 2];
                let mut dy = yi - self.positions[j * 2 + 1];
                let mut d2 = dx * dx + dy * dy;
                if d2 < 1e-6 {
                    // Coincident (or near): deterministic index-derived nudge so
                    // the pair separates the same way on every run.
                    let s = (i as f32) * 12.9898 + (j as f32) * 78.233;
                    dx = s.sin() * 1e-2;
                    dy = s.cos() * 1e-2;
                    d2 = dx * dx + dy * dy;
                    if d2 < 1e-9 {
                        dx = 1e-2;
                        dy = 0.0;
                        d2 = 1e-4;
                    }
                }
                let inv_d = 1.0 / d2.sqrt();
                let mag = charge / d2.max(soft2); // repulsive magnitude, softened
                let ux = dx * inv_d;
                let uy = dy * inv_d;
                fx[i] += ux * mag;
                fy[i] += uy * mag;
                fx[j] -= ux * mag;
                fy[j] -= uy * mag;
            }
        }
    }

    /// Barnes-Hut repulsion: one quadtree per tick, one walk per node.
    fn repulsion_barnes_hut(&self, theta: f32, fx: &mut [f32], fy: &mut [f32]) {
        let n = self.n;
        let Some(tree) = QuadTree::build(&self.positions, n) else {
            return;
        };
        let charge = self.config.charge;
        let soft2 = self.softening_sq();
        let theta2 = theta * theta;
        let mut stack: Vec<u32> = Vec::with_capacity(64);
        for i in 0..n {
            let xi = self.positions[i * 2];
            let yi = self.positions[i * 2 + 1];
            let (mut ax, mut ay) = (0.0f32, 0.0f32);
            stack.clear();
            stack.push(0);
            while let Some(c) = stack.pop() {
                let cell = &tree.cells[c as usize];
                if cell.is_leaf() {
                    for &j in &tree.points[cell.start as usize..(cell.start + cell.count) as usize]
                    {
                        let j = j as usize;
                        if j == i {
                            continue;
                        }
                        // Same pair as the exact loop: lower index is "i".
                        let (lo, hi) = if i < j { (i, j) } else { (j, i) };
                        let (dx, dy, d2) = pair_delta(
                            xi - self.positions[j * 2],
                            yi - self.positions[j * 2 + 1],
                            lo,
                            hi,
                            i == lo,
                        );
                        let mag = charge / d2.max(soft2);
                        let inv_d = 1.0 / d2.sqrt();
                        ax += dx * inv_d * mag;
                        ay += dy * inv_d * mag;
                    }
                    continue;
                }
                let inside = (xi - cell.cx).abs() <= cell.half && (yi - cell.cy).abs() <= cell.half;
                let m = cell.count as f32;
                let dx = xi - cell.sx / m;
                let dy = yi - cell.sy / m;
                let d2 = dx * dx + dy * dy;
                let w = cell.half * 2.0;
                if !inside && d2 >= 1e-6 && w * w < theta2 * d2 {
                    let inv_d = 1.0 / d2.sqrt();
                    let mag = charge * m / d2.max(soft2);
                    ax += dx * inv_d * mag;
                    ay += dy * inv_d * mag;
                } else {
                    for &ch in &cell.children {
                        if ch != NONE {
                            stack.push(ch);
                        }
                    }
                }
            }
            fx[i] += ax;
            fy[i] += ay;
        }
    }

    /// `true` once the annealing temperature has hit the floor.
    #[inline]
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Current annealing temperature (for HUD/telemetry, protocol `alpha`).
    #[inline]
    pub fn alpha(&self) -> f32 {
        self.alpha
    }

    /// On-screen node count of the loaded scope.
    #[inline]
    pub fn node_count(&self) -> usize {
        self.n
    }

    /// Directed edge count of the loaded scope.
    #[inline]
    pub fn edge_count(&self) -> usize {
        self.edge_a.len()
    }

    /// Pointer to the live interleaved position buffer (`*const f32`). JS builds
    /// a `Float32Array(memory.buffer, ptr, positions_len())` over it — zero copy.
    /// Valid until the next `load_csr` (which may reallocate).
    #[inline]
    pub fn positions_ptr(&self) -> *const f32 {
        self.positions.as_ptr()
    }

    /// Length of the position buffer in `f32` elements (`node_count * 2`).
    #[inline]
    pub fn positions_len(&self) -> usize {
        self.positions.len()
    }

    /// Copy live positions into `out` (the transferable ping-pong path, when
    /// `SharedArrayBuffer` is unavailable). Copies `min(len)` elements.
    pub fn write_positions_into(&self, out: &mut [f32]) {
        let k = out.len().min(self.positions.len());
        out[..k].copy_from_slice(&self.positions[..k]);
    }

    /* ── parameter setters (protocol SimParamKey) ─────────────────────────── */

    /// Set repulsion constant (positive = repulsive).
    pub fn set_charge(&mut self, v: f32) {
        if !v.is_finite() {
            return;
        }
        self.config.charge = v;
    }
    /// Set spring rest length.
    pub fn set_link_distance(&mut self, v: f32) {
        if !v.is_finite() {
            return;
        }
        self.config.link_distance = v;
    }
    /// Set spring stiffness.
    pub fn set_link_strength(&mut self, v: f32) {
        if !v.is_finite() {
            return;
        }
        self.config.link_strength = v;
    }
    /// Set velocity damping.
    pub fn set_damping(&mut self, v: f32) {
        if !v.is_finite() {
            return;
        }
        self.config.damping = v;
    }
    /// Set centre-gravity strength.
    pub fn set_center_gravity(&mut self, v: f32) {
        if !v.is_finite() {
            return;
        }
        self.config.center_gravity = v;
    }
    /// Set the Barnes-Hut opening angle. `theta <= 0` selects the exact
    /// all-pairs loop ([`Repulsion::Exact`]); the d3-force default is 0.9.
    pub fn set_theta(&mut self, v: f32) {
        if !v.is_finite() {
            return;
        }
        self.config.repulsion = Repulsion::from_theta(v);
    }
    /// Select the repulsion strategy explicitly.
    pub fn set_repulsion(&mut self, r: Repulsion) {
        self.config.repulsion = r;
    }
    /// Set the alpha floor.
    pub fn set_alpha_min(&mut self, v: f32) {
        if !v.is_finite() {
            return;
        }
        self.config.alpha_min = v;
    }
    /// Hold node `i` at `(x, y)` with zero velocity until [`unpin`](Self::unpin)
    /// or the next load. It keeps exerting forces, so its neighbours follow it.
    /// Returns `false`, changing nothing, for an index outside the loaded scope
    /// or a non-finite coordinate. Does not reheat; callers that want the
    /// layout to respond call [`reheat`](Self::reheat) as well.
    pub fn pin(&mut self, i: usize, x: f32, y: f32) -> bool {
        if i >= self.n || !x.is_finite() || !y.is_finite() {
            return false;
        }
        self.positions[i * 2] = x;
        self.positions[i * 2 + 1] = y;
        self.velocities[i * 2] = 0.0;
        self.velocities[i * 2 + 1] = 0.0;
        self.pinned[i] = true;
        true
    }

    /// Release node `i` back to the layout. Returns `false` for an index
    /// outside the loaded scope.
    pub fn unpin(&mut self, i: usize) -> bool {
        match self.pinned.get_mut(i) {
            Some(p) => {
                *p = false;
                true
            }
            None => false,
        }
    }

    /// `true` while node `i` is pinned; `false` for any index outside the scope.
    pub fn is_pinned(&self, i: usize) -> bool {
        self.pinned.get(i).copied().unwrap_or(false)
    }

    /// Re-heat the simulation: raise alpha and clear the finished flag so a
    /// settled scope resumes motion (protocol `reheat`).
    pub fn reheat(&mut self, v: f32) {
        if !v.is_finite() {
            return;
        }
        self.alpha = v.max(self.config.alpha_min);
        self.finished = self.n == 0;
    }

    /// Dispatch a `SimParamKey` string to the matching setter. Unknown keys are
    /// ignored. Mirrors `protocol.ts::SimParamKey`.
    pub fn set_param(&mut self, key: &str, val: f32) {
        match key {
            "charge" => self.set_charge(val),
            "linkDistance" => self.set_link_distance(val),
            "linkStrength" => self.set_link_strength(val),
            "damping" => self.set_damping(val),
            "centerGravity" => self.set_center_gravity(val),
            "alphaMin" => self.set_alpha_min(val),
            "theta" => self.set_theta(val),
            "reheat" => self.reheat(val),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "ngg1")]
    use crate::ngg1::worked_example;

    /// The same three-node, two-edge scope the NGG1 worked example encodes,
    /// loaded through the format-independent path so every test below runs in
    /// all feature configurations.
    fn loaded() -> CsrSimulation {
        let mut sim = CsrSimulation::new();
        sim.load_arrays(
            vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            vec![0, 0],
            vec![1, 2],
            vec![0, 1],
        );
        sim
    }

    #[test]
    fn load_arrays_seeds_scope() {
        let sim = loaded();
        assert_eq!(sim.node_count(), 3);
        assert_eq!(sim.edge_count(), 2);
        assert_eq!(sim.positions_len(), 6);
        assert_eq!(&sim.positions[..], &[0.0f32, 0.0, 1.0, 0.0, 0.0, 1.0][..]);
        assert!(!sim.is_finished());
    }

    #[test]
    #[should_panic(expected = "odd length")]
    fn load_arrays_rejects_odd_positions() {
        CsrSimulation::new().load_arrays(vec![0.0, 0.0, 1.0], vec![], vec![], vec![]);
    }

    #[test]
    #[should_panic(expected = "parallel")]
    fn load_arrays_rejects_ragged_edges() {
        CsrSimulation::new().load_arrays(vec![0.0, 0.0], vec![0], vec![], vec![]);
    }

    #[cfg(feature = "ngg1")]
    #[test]
    fn load_seeds_from_ngg1() {
        let mut sim = CsrSimulation::new();
        sim.load_csr(&worked_example()).unwrap();
        assert_eq!(sim.node_count(), 3);
        assert_eq!(sim.edge_count(), 2);
        assert_eq!(sim.positions_len(), 6);
        // Seeded positions match the NGG1 warm-start (before any tick).
        assert_eq!(&sim.positions[..], &[0.0f32, 0.0, 1.0, 0.0, 0.0, 1.0][..]);
        assert!(!sim.is_finished());
        // The two loaders agree on the resulting scope.
        assert_eq!(sim.positions, loaded().positions);
    }

    #[test]
    fn tick_moves_nodes_and_stays_finite() {
        let mut sim = loaded();
        let before = sim.positions.clone();
        for _ in 0..50 {
            sim.tick();
        }
        assert_ne!(before, sim.positions, "nodes must move under forces");
        for v in &sim.positions {
            assert!(v.is_finite(), "no NaN/inf may escape the sim");
        }
    }

    #[test]
    fn deterministic_given_seeds() {
        let mut a = loaded();
        let mut b = loaded();
        for _ in 0..200 {
            a.tick();
            b.tick();
        }
        assert_eq!(
            a.positions, b.positions,
            "same seeds ⇒ identical trajectory"
        );
        assert_eq!(a.alpha(), b.alpha());
    }

    #[test]
    fn auto_finishes_at_alpha_floor() {
        let mut sim = loaded();
        for _ in 0..1000 {
            sim.tick();
        }
        assert!(sim.is_finished());
        assert!(sim.alpha() <= sim.config.alpha_min + f32::EPSILON);
    }

    #[test]
    fn finished_tick_is_noop() {
        let mut sim = loaded();
        for _ in 0..1000 {
            sim.tick();
        }
        let frozen = sim.positions.clone();
        sim.tick();
        assert_eq!(frozen, sim.positions);
    }

    #[test]
    fn reheat_resumes_motion() {
        let mut sim = loaded();
        for _ in 0..1000 {
            sim.tick();
        }
        assert!(sim.is_finished());
        sim.reheat(1.0);
        assert!(!sim.is_finished());
        let before = sim.positions.clone();
        sim.tick();
        assert_ne!(before, sim.positions);
    }

    #[test]
    fn write_positions_into_matches_ptr_buffer() {
        let mut sim = loaded();
        for _ in 0..10 {
            sim.tick();
        }
        let mut out = vec![0.0f32; sim.positions_len()];
        sim.write_positions_into(&mut out);
        assert_eq!(out, sim.positions);
    }

    #[test]
    fn set_param_dispatch() {
        let mut sim = loaded();
        sim.set_param("charge", 999.0);
        sim.set_param("linkDistance", 42.0);
        sim.set_param("unknownKey", 1.0); // ignored, no panic
        assert_eq!(sim.config.charge, 999.0);
        assert_eq!(sim.config.link_distance, 42.0);
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            for key in [
                "charge",
                "linkDistance",
                "linkStrength",
                "damping",
                "centerGravity",
                "alphaMin",
                "theta",
                "reheat",
            ] {
                sim.set_param(key, bad);
            }
        }
        assert_eq!(sim.config.charge, 999.0);
        assert_eq!(sim.config.link_distance, 42.0);
        assert!(sim.alpha.is_finite());
    }

    #[cfg(feature = "ngg1")]
    #[test]
    fn load_csr_rejects_malformed() {
        let mut sim = CsrSimulation::new();
        // Pure error path (Ngg1Error) — no JsValue, safe under native cargo test.
        assert!(sim.load_csr(&[0u8; 8]).is_err());
        let mut truncated = worked_example();
        truncated.truncate(100);
        assert!(sim.load_csr(&truncated).is_err());
    }

    #[test]
    fn empty_scope_is_finished_and_safe() {
        let mut sim = CsrSimulation::new();
        assert!(sim.is_finished() || sim.node_count() == 0);
        sim.tick(); // must not panic
        assert_eq!(sim.positions_len(), 0);
    }

    #[test]
    fn positions_ptr_stable_across_ticks() {
        let mut sim = loaded();
        let p0 = sim.positions_ptr();
        for _ in 0..20 {
            sim.tick();
        }
        // No reallocation during ticking ⇒ the JS Float32Array view stays valid.
        assert_eq!(p0, sim.positions_ptr());
    }

    /// Deterministic xorshift layout in `[-300, 300]²`.
    fn random_sim(n: usize, seed: u32) -> CsrSimulation {
        let mut state = seed;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state as f32 / u32::MAX as f32) * 600.0 - 300.0
        };
        let pos: Vec<f32> = (0..n * 2).map(|_| next()).collect();
        let mut sim = CsrSimulation::new();
        sim.load_arrays(pos, vec![], vec![], vec![]);
        sim
    }

    fn forces(sim: &CsrSimulation) -> (Vec<f32>, Vec<f32>) {
        let mut fx = vec![0.0; sim.n];
        let mut fy = vec![0.0; sim.n];
        sim.repulsion_forces(&mut fx, &mut fy);
        (fx, fy)
    }

    #[test]
    fn barnes_hut_theta_half_within_two_percent_of_exact() {
        let mut sim = random_sim(300, 0x9E37_79B9);
        sim.set_repulsion(Repulsion::Exact);
        let (ex, ey) = forces(&sim);
        sim.set_theta(0.5);
        let (bx, by) = forces(&sim);
        let (mut err, mut norm) = (0.0f64, 0.0f64);
        for i in 0..300 {
            let (dx, dy) = ((bx[i] - ex[i]) as f64, (by[i] - ey[i]) as f64);
            err += (dx * dx + dy * dy).sqrt();
            norm += ((ex[i] as f64).powi(2) + (ey[i] as f64).powi(2)).sqrt();
        }
        let rel = err / norm;
        println!("barnes-hut theta 0.5 aggregate relative error = {rel:.5}");
        assert!(rel < 0.02, "relative error {rel}");
    }

    #[test]
    fn theta_zero_is_bit_identical_to_exact() {
        let mut a = random_sim(120, 7);
        let mut b = random_sim(120, 7);
        a.set_repulsion(Repulsion::Exact);
        b.set_theta(0.0);
        assert_eq!(b.config.repulsion, Repulsion::Exact);
        assert_eq!(forces(&a), forces(&b));
        for _ in 0..20 {
            a.tick();
            b.tick();
        }
        let bits = |s: &CsrSimulation| s.positions.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
        assert_eq!(bits(&a), bits(&b));
    }

    #[test]
    fn barnes_hut_layout_converges_and_stays_finite() {
        let mut sim = random_sim(200, 42);
        let a0 = sim.alpha();
        assert!(matches!(sim.config.repulsion, Repulsion::BarnesHut(t) if t == 0.9));
        for _ in 0..400 {
            sim.tick();
        }
        assert!(sim.alpha() < a0);
        assert!(sim.is_finished());
        assert!(sim.positions.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn coincident_and_single_nodes_are_safe() {
        for repulsion in [Repulsion::Exact, Repulsion::BarnesHut(0.9)] {
            let mut one = CsrSimulation::new();
            one.load_arrays(vec![5.0, 5.0], vec![], vec![], vec![]);
            one.set_repulsion(repulsion);
            let mut same = CsrSimulation::new();
            same.load_arrays(vec![1.0; 20], vec![], vec![], vec![]);
            same.set_repulsion(repulsion);
            for _ in 0..50 {
                one.tick();
                same.tick();
            }
            assert!(one.positions.iter().all(|v| v.is_finite()));
            assert!(same.positions.iter().all(|v| v.is_finite()));
            assert_ne!(
                same.positions[0], same.positions[2],
                "coincident nodes separate"
            );
        }
    }

    #[test]
    fn coincident_pair_forces_match_exact() {
        let mut sim = CsrSimulation::new();
        sim.load_arrays(vec![1.0, 1.0, 1.0, 1.0, 50.0, 3.0], vec![], vec![], vec![]);
        sim.set_repulsion(Repulsion::Exact);
        let exact = forces(&sim);
        sim.set_theta(0.9);
        let bh = forces(&sim);
        for k in 0..3 {
            assert!((exact.0[k] - bh.0[k]).abs() <= 1e-3 * exact.0[k].abs().max(1.0));
            assert!((exact.1[k] - bh.1[k]).abs() <= 1e-3 * exact.1[k].abs().max(1.0));
        }
    }

    /// A near-coincident seed pair (0.67 apart, as two published AI-domain classes
    /// were) must not be flung across the canvas: `charge / d²` is unbounded as
    /// `d → 0`, so without softening and a step cap tick 1 moved them ~2,700 units.
    #[test]
    fn near_coincident_seeds_stay_in_bounds() {
        for repulsion in [Repulsion::Exact, Repulsion::BarnesHut(0.9)] {
            let mut sim = random_sim(60, 99);
            sim.positions[0] = 10.0;
            sim.positions[1] = 10.0;
            sim.positions[2] = 10.67;
            sim.positions[3] = 10.0;
            sim.set_repulsion(repulsion);
            let before = sim.positions.clone();
            sim.tick();
            let max_step = 2.0 * sim.config.link_distance;
            for i in 0..sim.n {
                let d = ((sim.positions[i * 2] - before[i * 2]).powi(2)
                    + (sim.positions[i * 2 + 1] - before[i * 2 + 1]).powi(2))
                .sqrt();
                assert!(d <= max_step + 1e-3, "{repulsion:?}: node {i} stepped {d}");
            }
            assert_ne!(
                sim.positions[0], sim.positions[2],
                "the pair still separates"
            );
            while !sim.is_finished() {
                sim.tick();
            }
            let r =
                |i: usize| (sim.positions[i * 2].powi(2) + sim.positions[i * 2 + 1].powi(2)).sqrt();
            let mut radii: Vec<f32> = (0..sim.n).map(r).collect();
            radii.sort_by(f32::total_cmp);
            let median = radii[sim.n / 2];
            assert!(
                r(0) < 3.0 * median && r(1) < 3.0 * median,
                "{repulsion:?}: pair ended at {} / {} vs median {median}",
                r(0),
                r(1)
            );
        }
    }

    /// Three nodes in a chain; the middle one is dragged far to the right.
    fn chain() -> CsrSimulation {
        let mut sim = CsrSimulation::new();
        sim.load_arrays(
            vec![0.0, 0.0, 30.0, 0.0, 60.0, 0.0],
            vec![0, 1],
            vec![1, 2],
            vec![0, 0],
        );
        sim
    }

    #[test]
    fn a_pinned_node_holds_its_position_and_its_neighbours_follow() {
        let mut sim = chain();
        assert!(sim.pin(1, 400.0, 0.0));
        let before = sim.positions[0];
        for _ in 0..200 {
            sim.tick();
        }
        let p = &sim.positions;
        assert_eq!((p[2], p[3]), (400.0, 0.0), "pinned node must not move");
        assert!(
            p[0] > before + 100.0,
            "spring neighbour should follow, got {}",
            p[0]
        );
        assert!(sim.is_pinned(1));
    }

    #[test]
    fn unpinning_returns_the_node_to_the_layout() {
        let mut sim = chain();
        sim.pin(1, 400.0, 0.0);
        sim.tick();
        assert!(sim.unpin(1));
        assert!(!sim.is_pinned(1));
        sim.reheat(0.5);
        sim.tick();
        assert_ne!(sim.positions[2], 400.0, "an unpinned node moves again");
    }

    #[test]
    fn invalid_pins_are_refused_without_panicking() {
        let mut sim = chain();
        assert!(!sim.pin(3, 0.0, 0.0));
        assert!(!sim.pin(usize::MAX, 0.0, 0.0));
        assert!(!sim.pin(0, f32::NAN, 0.0));
        assert!(!sim.pin(0, 0.0, f32::INFINITY));
        assert!(!sim.unpin(3));
        assert!(!sim.is_pinned(3));
        assert_eq!(sim.positions[0], 0.0, "a refused pin leaves the node alone");
    }

    #[test]
    fn loading_a_new_scope_clears_every_pin() {
        let mut sim = chain();
        sim.pin(0, 5.0, 5.0);
        sim.load_arrays(
            vec![0.0, 0.0, 30.0, 0.0, 60.0, 0.0],
            vec![0],
            vec![1],
            vec![0],
        );
        assert!(!sim.is_pinned(0));
    }

    /// `cargo test --release -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn tick_timing_n2000_exact_vs_barnes_hut() {
        for (label, repulsion) in [
            ("exact", Repulsion::Exact),
            ("barnes-hut 0.9", Repulsion::BarnesHut(0.9)),
        ] {
            let mut sim = random_sim(2000, 1234);
            sim.set_repulsion(repulsion);
            sim.tick(); // warm-up
            let t = std::time::Instant::now();
            for _ in 0..20 {
                sim.tick();
            }
            println!("n=2000 {label}: {:?}/tick", t.elapsed() / 20);
        }
    }
}
