//! Simulation Grid module for AssyPlan Phase 3
//!
//! Generates all possible nodes and elements within a grid configuration.
//! All geometry/duplication/validity checks are performed ONCE here,
//! so the simulation engine can draw from a pre-validated pool without
//! repeating expensive checks every step.
//!
//! Coordinate convention (same as Phase 1/2):
//!   - Grid intersections: x = col_idx * dx, y = row_idx * dy, z = level_idx * dz
//!   - z=0 is ground level (no girders allowed at z=0)
//!   - Columns: same (x,y), consecutive z levels
//!   - Girders:  same z, consecutive (x,y) along one axis (no diagonal)
//! Node IDs: 1-indexed, sorted by (x asc, y asc, z asc)  — matches Phase 1 ordering
//! Element IDs: 1-indexed, Columns first (sorted by id), then Girders (sorted by id)

use std::collections::HashMap;

use crate::stability::{StabilityElement, StabilityNode};

// ============================================================================
// Public API
// ============================================================================

/// Pre-validated pool of all possible nodes and elements for a given grid.
#[derive(Debug, Clone)]
pub struct SimGrid {
    /// All grid nodes (1-indexed IDs)
    pub nodes: Vec<StabilityNode>,
    /// All valid elements: columns first, then girders (1-indexed IDs)
    pub elements: Vec<StabilityElement>,
    /// Fast lookup: (xi, yi, zi) -> node_id
    pub node_index: HashMap<(usize, usize, usize), i32>,
    /// nx, ny, nz dimensions from the config
    pub nx: usize,
    pub ny: usize,
    /// Number of floor levels (z_levels from GridConfig)
    pub nz: usize,
}

impl SimGrid {
    /// Build the full pre-validated pool from a grid configuration.
    ///
    /// `nx`  — number of grid lines in x direction (≥ 2 for any girder/column)
    /// `ny`  — number of grid lines in y direction (≥ 2)
    /// `nz`  — number of z levels including z=0 ground (≥ 2 for any column)
    /// `dx`  — spacing between x grid lines
    /// `dy`  — spacing between y grid lines
    /// `dz`  — floor height (spacing between z levels)
    pub fn new(nx: usize, ny: usize, nz: usize, dx: f64, dy: f64, dz: f64) -> Self {
        let nx = nx.max(2);
        let ny = ny.max(2);
        let nz = nz.max(2);

        // ── 1. Generate nodes sorted by (x asc, y asc, z asc) ──────────────
        // x iterates slowest to match Phase 1 ordering: z→x→y priority
        // But Phase 1 sorts by (x,y,z) with z=fastest? Let's match exactly:
        //   sort key = (z asc, x asc, y asc)  ← Phase 1: z→x→y priority
        let mut raw_nodes: Vec<(usize, usize, usize, f64, f64, f64)> = Vec::new();
        for xi in 0..nx {
            for yi in 0..ny {
                for zi in 0..nz {
                    let x = xi as f64 * dx;
                    let y = yi as f64 * dy;
                    let z = zi as f64 * dz;
                    raw_nodes.push((xi, yi, zi, x, y, z));
                }
            }
        }
        // Sort by z asc, then x asc, then y asc  (matches Phase 1 node ordering)
        raw_nodes.sort_by(|a, b| a.2.cmp(&b.2).then(a.0.cmp(&b.0)).then(a.1.cmp(&b.1)));

        let mut nodes: Vec<StabilityNode> = Vec::with_capacity(raw_nodes.len());
        let mut node_index: HashMap<(usize, usize, usize), i32> =
            HashMap::with_capacity(raw_nodes.len());

        for (seq, (xi, yi, zi, x, y, z)) in raw_nodes.into_iter().enumerate() {
            let id = (seq + 1) as i32; // 1-indexed
            node_index.insert((xi, yi, zi), id);
            nodes.push(StabilityNode { id, x, y, z });
        }

        // ── 2. Generate elements ─────────────────────────────────────────────
        // Columns: (xi,yi,zi) → (xi,yi,zi+1)  for zi in 0..nz-1
        // Girders (x-dir): (xi,yi,zi) → (xi+1,yi,zi) for zi ≥ 1 (no ground girder)
        // Girders (y-dir): (xi,yi,zi) → (xi,yi+1,zi) for zi ≥ 1
        // Element IDs: columns first (sorted by node_i_id then node_j_id), girders after
        let mut columns: Vec<StabilityElement> = Vec::new();
        let mut girders: Vec<StabilityElement> = Vec::new();

        for xi in 0..nx {
            for yi in 0..ny {
                for zi in 0..(nz - 1) {
                    // Column: zi → zi+1
                    let ni = *node_index.get(&(xi, yi, zi)).unwrap();
                    let nj = *node_index.get(&(xi, yi, zi + 1)).unwrap();
                    columns.push(StabilityElement {
                        id: 0, // assigned later
                        node_i_id: ni,
                        node_j_id: nj,
                        member_type: "Column".to_string(),
                    });
                }
            }
        }

        for zi in 1..nz {
            // No girders at z=0 (zi==0 is ground level)
            for xi in 0..nx {
                for yi in 0..ny {
                    // x-direction girder: (xi,yi,zi) → (xi+1,yi,zi)
                    if xi + 1 < nx {
                        let ni = *node_index.get(&(xi, yi, zi)).unwrap();
                        let nj = *node_index.get(&(xi + 1, yi, zi)).unwrap();
                        girders.push(StabilityElement {
                            id: 0,
                            node_i_id: ni,
                            node_j_id: nj,
                            member_type: "Girder".to_string(),
                        });
                    }
                    // y-direction girder: (xi,yi,zi) → (xi,yi+1,zi)
                    if yi + 1 < ny {
                        let ni = *node_index.get(&(xi, yi, zi)).unwrap();
                        let nj = *node_index.get(&(xi, yi + 1, zi)).unwrap();
                        girders.push(StabilityElement {
                            id: 0,
                            node_i_id: ni,
                            node_j_id: nj,
                            member_type: "Girder".to_string(),
                        });
                    }
                }
            }
        }

        // Sort columns by (node_i_id, node_j_id) for determinism
        columns.sort_by_key(|e| (e.node_i_id, e.node_j_id));
        girders.sort_by_key(|e| (e.node_i_id, e.node_j_id));

        // Assign 1-indexed IDs: columns 1..N_col, girders N_col+1..total
        let mut elements: Vec<StabilityElement> = Vec::with_capacity(columns.len() + girders.len());
        for (i, mut col) in columns.into_iter().enumerate() {
            col.id = (i + 1) as i32;
            elements.push(col);
        }
        let col_count = elements.len();
        for (i, mut gdr) in girders.into_iter().enumerate() {
            gdr.id = (col_count + i + 1) as i32;
            elements.push(gdr);
        }

        Self {
            nodes,
            elements,
            node_index,
            nx,
            ny,
            nz,
        }
    }

    // ── Convenience accessors ────────────────────────────────────────────────

    /// Look up node ID by grid index (0-based). Returns None if out of range.
    pub fn node_id_at(&self, xi: usize, yi: usize, zi: usize) -> Option<i32> {
        self.node_index.get(&(xi, yi, zi)).copied()
    }

    /// Total column count
    pub fn column_count(&self) -> usize {
        self.elements
            .iter()
            .filter(|e| e.member_type == "Column")
            .count()
    }

    /// Total girder count
    pub fn girder_count(&self) -> usize {
        self.elements
            .iter()
            .filter(|e| e.member_type == "Girder")
            .count()
    }

    /// All columns (reference slice is borrowed, no clone)
    pub fn columns(&self) -> impl Iterator<Item = &StabilityElement> {
        self.elements.iter().filter(|e| e.member_type == "Column")
    }

    /// All girders
    pub fn girders(&self) -> impl Iterator<Item = &StabilityElement> {
        self.elements.iter().filter(|e| e.member_type == "Girder")
    }

    /// Find the column element that starts at grid position (xi, yi, zi).
    /// Returns element ID or None.
    pub fn column_starting_at(&self, xi: usize, yi: usize, zi: usize) -> Option<i32> {
        let ni = self.node_id_at(xi, yi, zi)?;
        let nj = self.node_id_at(xi, yi, zi + 1)?;
        self.elements
            .iter()
            .find(|e| e.member_type == "Column" && e.node_i_id == ni && e.node_j_id == nj)
            .map(|e| e.id)
    }

    /// Find a girder connecting two adjacent nodes (order-insensitive).
    pub fn girder_between(&self, na: i32, nb: i32) -> Option<i32> {
        self.elements
            .iter()
            .find(|e| {
                e.member_type == "Girder"
                    && ((e.node_i_id == na && e.node_j_id == nb)
                        || (e.node_i_id == nb && e.node_j_id == na))
            })
            .map(|e| e.id)
    }

    /// Return the node coordinates by node ID.
    pub fn node_coords(&self, id: i32) -> Option<(f64, f64, f64)> {
        self.nodes
            .iter()
            .find(|n| n.id == id)
            .map(|n| (n.x, n.y, n.z))
    }

    /// Return columns adjacent (sharing a node) to an already-installed column.
    /// "Adjacent" means same floor, differing by exactly 1 in xi or yi.
    pub fn adjacent_columns(&self, col_id: i32, floor_zi: usize) -> Vec<i32> {
        // Find grid position of this column
        let col = match self.elements.iter().find(|e| e.id == col_id) {
            Some(e) => e,
            None => return Vec::new(),
        };
        // Determine xi,yi from node_i (lower node) position
        let ni_coords = match self.node_coords(col.node_i_id) {
            Some(c) => c,
            None => return Vec::new(),
        };
        // Reverse-lookup xi, yi, zi from node coordinates
        let xi = (ni_coords.0 / (self.nodes[1].x - self.nodes[0].x + 1e-9)).round() as usize;
        let _ = xi; // unused warning suppression
                    // Simpler: iterate node_index for matching coords
        let dx = if self.nx > 1 {
            self.nodes
                .iter()
                .filter(|n| n.y == ni_coords.1 && n.z == ni_coords.2)
                .map(|n| n.x)
                .collect::<Vec<_>>()
                .windows(2)
                .filter_map(|w| {
                    let d = w[1] - w[0];
                    if d > 0.0 {
                        Some(d)
                    } else {
                        None
                    }
                })
                .next()
                .unwrap_or(1.0)
        } else {
            1.0
        };
        let dy = if self.ny > 1 {
            self.nodes
                .iter()
                .filter(|n| n.x == ni_coords.0 && n.z == ni_coords.2)
                .map(|n| n.y)
                .collect::<Vec<_>>()
                .windows(2)
                .filter_map(|w| {
                    let d = w[1] - w[0];
                    if d > 0.0 {
                        Some(d)
                    } else {
                        None
                    }
                })
                .next()
                .unwrap_or(1.0)
        } else {
            1.0
        };

        let xi_f = (ni_coords.0 / dx).round() as i32;
        let yi_f = (ni_coords.1 / dy).round() as i32;

        let mut result = Vec::new();
        for (dxi, dyi) in [(-1i32, 0), (1, 0), (0, -1i32), (0, 1)] {
            let nxi = xi_f + dxi;
            let nyi = yi_f + dyi;
            if nxi < 0 || nyi < 0 {
                continue;
            }
            if let Some(eid) = self.column_starting_at(nxi as usize, nyi as usize, floor_zi) {
                result.push(eid);
            }
        }
        result
    }
}

// ============================================================================
// Grid statistics (used by UI for info display)
// ============================================================================
#[derive(Debug, Clone)]
pub struct GridStats {
    pub node_count: usize,
    pub column_count: usize,
    pub girder_count: usize,
    pub total_element_count: usize,
    pub floor_count: usize,
}

impl SimGrid {
    pub fn stats(&self) -> GridStats {
        let col = self.column_count();
        let gdr = self.girder_count();
        GridStats {
            node_count: self.nodes.len(),
            column_count: col,
            girder_count: gdr,
            total_element_count: col + gdr,
            floor_count: self.nz.saturating_sub(1),
        }
    }
}

// ============================================================================
// Unit tests
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn make_2x2x2() -> SimGrid {
        // 2×2 grid, 2 z-levels (1 floor)
        SimGrid::new(2, 2, 2, 6000.0, 6000.0, 4000.0)
    }

    #[test]
    fn test_node_count() {
        let g = make_2x2x2();
        // 2×2×2 = 8 nodes
        assert_eq!(g.nodes.len(), 8);
    }

    #[test]
    fn test_node_ids_start_at_1() {
        let g = make_2x2x2();
        let ids: Vec<i32> = g.nodes.iter().map(|n| n.id).collect();
        assert!(ids.contains(&1));
        assert!(!ids.contains(&0));
    }

    #[test]
    fn test_column_count() {
        let g = make_2x2x2();
        // 2×2 grid = 4 columns per floor, 1 floor
        assert_eq!(g.column_count(), 4);
    }

    #[test]
    fn test_no_ground_girder() {
        let g = make_2x2x2();
        let ground_z = 0.0_f64;
        for elem in g.girders() {
            let (_, _, z_i) = g.node_coords(elem.node_i_id).unwrap();
            assert!(
                (z_i - ground_z).abs() > 1e-9,
                "Girder at ground z=0 found (id={})",
                elem.id
            );
        }
    }

    #[test]
    fn test_girder_count_2x2x2() {
        let g = make_2x2x2();
        // At z=1 (zi=1): x-dir: 2×1=2 (per row, 2 rows) → 2, y-dir: 1×2=2 (per col, 2 cols) → 2. Wait:
        // x-dir girders at floor z level: for yi in 0..2, xi in 0..1 → 2 girders
        // y-dir girders at floor z level: for xi in 0..2, yi in 0..1 → 2 girders
        // Total 4 girders
        assert_eq!(g.girder_count(), 4);
    }

    #[test]
    fn test_element_ids_start_at_1() {
        let g = make_2x2x2();
        let ids: Vec<i32> = g.elements.iter().map(|e| e.id).collect();
        assert!(ids.contains(&1));
        assert!(!ids.contains(&0));
    }

    #[test]
    fn test_element_ids_unique() {
        let g = SimGrid::new(3, 3, 3, 6000.0, 6000.0, 4000.0);
        let ids: Vec<i32> = g.elements.iter().map(|e| e.id).collect();
        let unique: std::collections::HashSet<i32> = ids.iter().cloned().collect();
        assert_eq!(ids.len(), unique.len());
    }

    #[test]
    fn test_columns_before_girders_in_ids() {
        let g = make_2x2x2();
        let max_col_id = g.columns().map(|e| e.id).max().unwrap_or(0);
        let min_gdr_id = g.girders().map(|e| e.id).min().unwrap_or(i32::MAX);
        assert!(
            max_col_id < min_gdr_id,
            "columns should have lower IDs than girders"
        );
    }

    #[test]
    fn test_node_index_lookup() {
        let g = make_2x2x2();
        // (0,0,0) must exist
        assert!(g.node_id_at(0, 0, 0).is_some());
        // out of range
        assert!(g.node_id_at(9, 9, 9).is_none());
    }

    #[test]
    fn test_column_starting_at() {
        let g = make_2x2x2();
        let col = g.column_starting_at(0, 0, 0);
        assert!(col.is_some(), "Column at (0,0,0) should exist");
    }

    #[test]
    fn test_larger_grid_stats() {
        let g = SimGrid::new(4, 4, 3, 6000.0, 6000.0, 4000.0);
        let s = g.stats();
        // 4×4×3 = 48 nodes
        assert_eq!(s.node_count, 48);
        // Columns: 4×4×2 = 32
        assert_eq!(s.column_count, 32);
        // Girders per floor: x-dir 3×4=12, y-dir 4×3=12 → 24 per floor, 2 floors = 48
        assert_eq!(s.girder_count, 48);
        // 2 floors (nz-1)
        assert_eq!(s.floor_count, 2);
    }
}
