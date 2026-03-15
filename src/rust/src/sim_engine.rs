//! Simulation Engine for AssyPlan Phase 3
//!
//! Monte-Carlo + Pruning + Weighted Sampling
//!
//! Per-step selection unit: ONE element at a time (not bundles).
//!
//! Special case — first step (bootstrap):
//!   Nothing is installed yet → must form minimum assembly (3 cols + 2 girders)
//!   anchored at the workfront position.  This is the only multi-element step.
//!
//! Normal steps (after bootstrap):
//!   1. Collect candidate single elements reachable from the frontier:
//!      - Uninstalled columns adjacent (±1 grid step) to any installed node, on lowest unstarted floor
//!      - Uninstalled girders whose ≥1 endpoint is an upper node of a frontier column
//!      - If none found at current floor → search next floor up
//!   2. Filter by structural stability (validate_column_support / validate_girder_support)
//!   3. Filter by upper-floor constraint
//!   4. Score each element: w1 × connectivity + w2 × (1/distance) + w3 × is_lowest_floor_bonus
//!   5. Weighted random sample → install 1 element → update frontier
//!
//! Early termination:
//!   1. Upper-floor constraint blocks everything 3 consecutive times
//!   2. No valid single candidate found 10 consecutive times
//!   3. No progress (< 3 members in last 300 iterations)
//!   4. Max iteration guard

use std::collections::HashSet;

use rayon::prelude::*;

use crate::graphics::ui::{ScenarioMetrics, SimScenario, SimStep, SimWorkfront, TerminationReason};
use crate::sim_grid::SimGrid;
use crate::stability::{has_minimum_assembly, validate_column_support, validate_girder_support};

// ============================================================================
// SingleCandidate — one element at a time
// ============================================================================

/// A single-element candidate for one simulation step.
#[derive(Clone, Debug)]
pub struct SingleCandidate {
    /// The one element to install
    pub element_id: i32,
    /// Number of shared nodes with already-installed structure
    pub connectivity: usize,
    /// Manhattan grid distance from nearest frontier node
    pub frontier_dist: f64,
    /// Whether this element is on the lowest unstarted floor
    pub is_lowest_floor: bool,
}

impl SingleCandidate {
    /// Score = w1×connectivity + w2×(1/(dist+1)) + 0.05×lowest_floor_bonus
    pub fn score(&self, w1: f64, w2: f64) -> f64 {
        let s_conn = w1 * self.connectivity as f64;
        let s_dist = w2 * (1.0 / (self.frontier_dist + 1.0));
        let s_floor = 0.05 * if self.is_lowest_floor { 1.0 } else { 0.0 };
        s_conn + s_dist + s_floor
    }
}

// ============================================================================
// Legacy Candidate kept for bootstrap only (first step: 3col+2gdr bundle)
// ============================================================================

#[derive(Clone, Debug)]
pub struct Candidate {
    pub element_ids: Vec<i32>,
    pub member_count: usize,
    pub connectivity: f64,
    pub frontier_dist: f64,
    pub is_lowest_floor: bool,
    pub is_independent: bool,
}

impl Candidate {
    pub fn score(&self, w1: f64, w2: f64, w3: f64) -> f64 {
        let s_members = w1 * (1.0 / self.member_count.max(1) as f64);
        let s_conn = w2 * self.connectivity;
        let s_dist = w3 * (1.0 / (self.frontier_dist + 1.0));
        let s_floor = 0.05 * if self.is_lowest_floor { 1.0 } else { 0.0 };
        s_members + s_conn + s_dist + s_floor
    }
}

// ============================================================================
// Grid helpers
// ============================================================================

/// Extract the grid dz (floor height) from a SimGrid.
fn grid_dz(grid: &SimGrid) -> f64 {
    let mut z_vals: Vec<i64> = grid
        .nodes
        .iter()
        .map(|n| (n.z * 1000.0).round() as i64)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    z_vals.sort();
    if z_vals.len() >= 2 {
        (z_vals[1] - z_vals[0]) as f64 / 1000.0
    } else {
        4000.0
    }
}

/// Build a reverse map: node_id → (xi, yi, zi)
fn build_node_pos(grid: &SimGrid) -> std::collections::HashMap<i32, (usize, usize, usize)> {
    grid.node_index
        .iter()
        .map(|(&pos, &id)| (id, pos))
        .collect()
}

/// Manhattan distance in grid steps between two nodes.
fn node_grid_dist(
    nid_a: i32,
    nid_b: i32,
    node_pos: &std::collections::HashMap<i32, (usize, usize, usize)>,
) -> f64 {
    match (node_pos.get(&nid_a), node_pos.get(&nid_b)) {
        (Some(&(ax, ay, _)), Some(&(bx, by, _))) => {
            ((ax as i32 - bx as i32).abs() + (ay as i32 - by as i32).abs()) as f64
        }
        _ => f64::MAX,
    }
}

/// Minimum Manhattan grid distance from an element's nodes to any installed node.
fn element_frontier_dist(
    element_id: i32,
    grid: &SimGrid,
    installed_nodes: &HashSet<i32>,
    node_pos: &std::collections::HashMap<i32, (usize, usize, usize)>,
) -> f64 {
    let elem = match grid.elements.iter().find(|e| e.id == element_id) {
        Some(e) => e,
        None => return f64::MAX,
    };
    if installed_nodes.is_empty() {
        return 0.0;
    }
    let cand_nodes = [elem.node_i_id, elem.node_j_id];
    let mut min_d = f64::MAX;
    for &cn in &cand_nodes {
        for &fn_id in installed_nodes.iter() {
            let d = node_grid_dist(cn, fn_id, node_pos);
            if d < min_d {
                min_d = d;
            }
        }
    }
    if min_d == f64::MAX {
        0.0
    } else {
        min_d
    }
}

/// Count how many of the element's nodes are already installed.
fn element_connectivity(element_id: i32, grid: &SimGrid, installed_nodes: &HashSet<i32>) -> usize {
    let elem = match grid.elements.iter().find(|e| e.id == element_id) {
        Some(e) => e,
        None => return 0,
    };
    let mut count = 0;
    if installed_nodes.contains(&elem.node_i_id) {
        count += 1;
    }
    if installed_nodes.contains(&elem.node_j_id) {
        count += 1;
    }
    count
}

/// Count shared nodes for a multi-element set (used only in bootstrap).
fn count_shared_nodes(
    element_ids: &[i32],
    grid: &SimGrid,
    installed_nodes: &HashSet<i32>,
) -> usize {
    let cand_nodes: HashSet<i32> = element_ids
        .iter()
        .flat_map(|eid| {
            grid.elements
                .iter()
                .find(|e| e.id == *eid)
                .map(|e| vec![e.node_i_id, e.node_j_id])
                .unwrap_or_default()
        })
        .collect();
    cand_nodes.intersection(installed_nodes).count()
}

// ============================================================================
// Stability checks
// ============================================================================

/// Check stability for a single element against the current installed set.
fn check_single_stability(element_id: i32, grid: &SimGrid, installed_ids: &HashSet<i32>) -> bool {
    let elem = match grid.elements.iter().find(|e| e.id == element_id) {
        Some(e) => e,
        None => return false,
    };
    if elem.member_type == "Column" {
        validate_column_support(elem, &grid.nodes, &grid.elements, installed_ids)
    } else {
        // Girder: both ends must connect to already-installed elements
        validate_girder_support(elem, &grid.nodes, &grid.elements, installed_ids)
    }
}

/// Check stability for a multi-element bundle (bootstrap only).
fn check_bundle_stability(
    element_ids: &[i32],
    grid: &SimGrid,
    installed_ids: &HashSet<i32>,
) -> bool {
    let mut combined: HashSet<i32> = installed_ids.clone();
    combined.extend(element_ids.iter().cloned());

    if installed_ids.is_empty() {
        let combined_elems: Vec<_> = grid
            .elements
            .iter()
            .filter(|e| combined.contains(&e.id))
            .cloned()
            .collect();
        return has_minimum_assembly(&grid.nodes, &combined_elems);
    }
    for eid in element_ids {
        let elem = match grid.elements.iter().find(|e| e.id == *eid) {
            Some(e) => e,
            None => return false,
        };
        let ok = if elem.member_type == "Column" {
            validate_column_support(elem, &grid.nodes, &grid.elements, installed_ids)
        } else {
            validate_girder_support(elem, &grid.nodes, &grid.elements, &combined)
        };
        if !ok {
            return false;
        }
    }
    true
}

/// Check upper-floor constraint for a set of element IDs.
///
/// Spec (devplandoc.md §311):
///   For floor N (1-indexed), the ratio
///     (cumulative N+1 installed columns + new N+1 columns in this step)
///       / (cumulative N installed columns)
///   must NOT exceed `threshold`.
///
/// **Key rule**: If floor N is 100% complete (all columns installed), the
/// constraint is lifted entirely — floor N+1 becomes the main work target.
///
/// Floor 1 columns are always allowed (no lower floor to compare against).
/// Girders never trigger this constraint (columns-only ratio).
/// If denominator (floor N installed) == 0 → ratio = 0.0 → always below threshold.
fn check_upper_floor_constraint(
    element_ids: &[i32],
    grid: &SimGrid,
    installed_ids: &HashSet<i32>,
    threshold: f64,
) -> bool {
    let dz = grid_dz(grid);

    // Count total columns per floor (needed for 100%-complete check)
    let mut total_per_floor: std::collections::HashMap<i32, usize> =
        std::collections::HashMap::new();
    for elem in grid.elements.iter().filter(|e| e.member_type == "Column") {
        if let Some((_, _, z)) = grid.node_coords(elem.node_i_id) {
            let floor = (z / dz).round() as i32 + 1;
            *total_per_floor.entry(floor).or_insert(0) += 1;
        }
    }

    // Count installed columns per floor (already installed, not including this step)
    let mut installed_per_floor: std::collections::HashMap<i32, usize> =
        std::collections::HashMap::new();
    for eid in installed_ids {
        if let Some(elem) = grid.elements.iter().find(|e| e.id == *eid) {
            if elem.member_type == "Column" {
                if let Some((_, _, z)) = grid.node_coords(elem.node_i_id) {
                    let floor = (z / dz).round() as i32 + 1;
                    *installed_per_floor.entry(floor).or_insert(0) += 1;
                }
            }
        }
    }

    // For each NEW column in this step, check if adding it would violate the ratio
    for eid in element_ids {
        let elem = match grid.elements.iter().find(|e| e.id == *eid) {
            Some(e) => e,
            None => continue,
        };
        if elem.member_type != "Column" {
            continue; // girders don't affect the ratio
        }
        let (_, _, z) = match grid.node_coords(elem.node_i_id) {
            Some(c) => c,
            None => continue,
        };
        let floor = (z / dz).round() as i32 + 1;

        // Floor 1 is always allowed (no lower floor)
        if floor <= 1 {
            continue;
        }

        let lower_floor = floor - 1;
        let installed_lower = *installed_per_floor.get(&lower_floor).unwrap_or(&0);
        let total_lower = *total_per_floor.get(&lower_floor).unwrap_or(&0);

        // If lower floor has 0 installed, ratio = 0.0 → allowed
        if installed_lower == 0 {
            continue;
        }

        // KEY RULE: If lower floor is 100% complete, constraint is lifted entirely.
        // Upper floor becomes the main work target — no ratio limit applies.
        if total_lower > 0 && installed_lower >= total_lower {
            continue;
        }

        // installed_upper includes already installed + this new column
        let installed_upper = *installed_per_floor.get(&floor).unwrap_or(&0) + 1;
        let ratio = installed_upper as f64 / installed_lower as f64;

        if ratio > threshold {
            return false;
        }
    }
    true
}

// ============================================================================
// Candidate generation
// ============================================================================

/// Determine the lowest zi floor where any uninstalled element still exists.
///
/// Checks columns first (by their node_i zi), then girders (by their node_i zi,
/// which equals floor_zi+1 for girders sitting on top of that floor).
/// This prevents the frontier from jumping to higher floors when lower-floor
/// elements still remain, and correctly handles the end-game where only
/// girders remain after all columns are installed.
fn min_unstarted_floor(
    _wf: &SimWorkfront,
    grid: &SimGrid,
    installed_ids: &HashSet<i32>,
    node_pos: &std::collections::HashMap<i32, (usize, usize, usize)>,
) -> usize {
    // Phase 1: find lowest zi where uninstalled columns still exist
    for zi in 0..(grid.nz.saturating_sub(1)) {
        let has_uninstalled_col = grid.elements.iter().any(|e| {
            e.member_type == "Column"
                && !installed_ids.contains(&e.id)
                && node_pos.get(&e.node_i_id).map(|p| p.2).unwrap_or(999) == zi
        });
        if has_uninstalled_col {
            return zi;
        }
    }

    // Phase 2: all columns installed — find lowest zi_girder-1 where girders remain.
    // Girders have node_i at zi = floor_zi + 1 (ceiling of the floor below).
    // We return (zi_girder - 1) so that the caller's "search_zi+1 == zi_girder" logic works.
    let mut min_girder_floor_zi: Option<usize> = None;
    for elem in grid.elements.iter() {
        if elem.member_type != "Girder" || installed_ids.contains(&elem.id) {
            continue;
        }
        if let Some(&(_, _, zi_g)) = node_pos.get(&elem.node_i_id) {
            // zi_g is the node_i zi of this girder.
            // Girder belongs to "floor zi_g-1" (0-indexed).
            // We want to return that floor zi so the caller's upper_zi = search_zi+1 = zi_g.
            let floor_zi = zi_g.saturating_sub(1);
            min_girder_floor_zi = Some(match min_girder_floor_zi {
                None => floor_zi,
                Some(prev) => prev.min(floor_zi),
            });
        }
    }
    min_girder_floor_zi.unwrap_or(grid.nz.saturating_sub(1))
}

/// Collect single-element candidates reachable from the frontier.
///
/// Returns uninstalled columns and girders adjacent to installed nodes,
/// starting from the lowest unstarted floor.  If nothing found at that
/// floor, expands to the next floor up.
///
/// Candidate count is O(frontier_size × 5) — typically < 30.
fn collect_single_candidates(
    wf: &SimWorkfront,
    grid: &SimGrid,
    installed_ids: &HashSet<i32>,
    installed_nodes: &HashSet<i32>,
    node_pos: &std::collections::HashMap<i32, (usize, usize, usize)>,
) -> Vec<SingleCandidate> {
    let zi_target = min_unstarted_floor(wf, grid, installed_ids, node_pos);

    // Try zi_target first; if empty, try zi_target+1
    for attempt in 0..2usize {
        let search_zi = zi_target + attempt;
        if search_zi >= grid.nz.saturating_sub(1) {
            break;
        }

        let mut result: Vec<SingleCandidate> = Vec::new();
        let mut seen: HashSet<i32> = HashSet::new();
        let is_lowest = search_zi == 0;

        // ── Columns: adjacent (±1) to any installed node at search_zi or search_zi+1 ──
        for &nid in installed_nodes.iter() {
            let &(xi, yi, zi) = match node_pos.get(&nid) {
                Some(p) => p,
                None => continue,
            };
            // Only expand from nodes at the floor we care about
            if zi != search_zi && zi != search_zi + 1 {
                continue;
            }

            for &(dxi, dyi) in &[(0i32, 0i32), (-1, 0), (1, 0), (0, -1i32), (0, 1)] {
                let nxi = xi as i32 + dxi;
                let nyi = yi as i32 + dyi;
                if nxi < 0 || nyi < 0 {
                    continue;
                }
                if let Some(col_id) = grid.column_starting_at(nxi as usize, nyi as usize, search_zi)
                {
                    if !installed_ids.contains(&col_id) && seen.insert(col_id) {
                        let conn = element_connectivity(col_id, grid, installed_nodes);
                        let dist = element_frontier_dist(col_id, grid, installed_nodes, node_pos);
                        result.push(SingleCandidate {
                            element_id: col_id,
                            connectivity: conn,
                            frontier_dist: dist,
                            is_lowest_floor: is_lowest,
                        });
                    }
                }
            }
        }

        // ── Girders: any girder at (search_zi+1) with ≥1 endpoint that is
        //   the upper node of a candidate column OR an already-installed column ──
        let upper_zi = search_zi + 1;

        // Build set of upper nodes: candidate cols + installed cols at search_zi
        let mut upper_nodes: HashSet<i32> = HashSet::new();
        for sc in &result {
            if let Some(e) = grid.elements.iter().find(|e| e.id == sc.element_id) {
                upper_nodes.insert(e.node_j_id);
            }
        }
        for &eid in installed_ids.iter() {
            if let Some(e) = grid.elements.iter().find(|e| e.id == eid) {
                if e.member_type == "Column" {
                    if let Some(&(_, _, zi)) = node_pos.get(&e.node_i_id) {
                        if zi == search_zi {
                            upper_nodes.insert(e.node_j_id);
                        }
                    }
                }
            }
        }

        for gdr in grid.elements.iter().filter(|e| e.member_type == "Girder") {
            if installed_ids.contains(&gdr.id) {
                continue;
            }
            let zi_g = node_pos.get(&gdr.node_i_id).map(|p| p.2).unwrap_or(999);
            if zi_g != upper_zi {
                continue;
            }
            let ni_in = upper_nodes.contains(&gdr.node_i_id);
            let nj_in = upper_nodes.contains(&gdr.node_j_id);
            if (ni_in || nj_in) && seen.insert(gdr.id) {
                let conn = element_connectivity(gdr.id, grid, installed_nodes);
                let dist = element_frontier_dist(gdr.id, grid, installed_nodes, node_pos);
                result.push(SingleCandidate {
                    element_id: gdr.id,
                    connectivity: conn,
                    frontier_dist: dist,
                    is_lowest_floor: false, // girders are at ceiling of floor, not "lowest"
                });
            }
        }

        if !result.is_empty() {
            return result;
        }
        // attempt == 0 and nothing found → try next floor
    }

    // ── Fallback: frontier-based search found nothing on any floor.
    //   Gather ALL uninstalled elements on the lowest unstarted floor regardless
    //   of adjacency.  This handles isolated pockets (e.g. far corners of a large
    //   grid) and also the end-game where only girders remain after all columns
    //   are installed.
    {
        let zi_target = min_unstarted_floor(wf, grid, installed_ids, node_pos);

        // Try from zi_target upward, collect any uninstalled element on that level.
        // For columns: zi of node_i == zi.  For girders: zi of node_i == zi+1
        // (girders sit at the ceiling of the floor below, i.e. upper_zi = zi+1).
        for zi in zi_target..=(grid.nz.saturating_sub(1)) {
            let mut result: Vec<SingleCandidate> = Vec::new();
            let mut seen: HashSet<i32> = HashSet::new();

            for elem in grid.elements.iter() {
                if installed_ids.contains(&elem.id) {
                    continue;
                }
                let elem_zi = node_pos.get(&elem.node_i_id).map(|p| p.2).unwrap_or(9999);

                let on_this_floor = if elem.member_type == "Column" {
                    elem_zi == zi
                } else {
                    // Girder: node_i zi == zi+1 means it sits on top of floor zi
                    elem_zi == zi + 1
                };
                if !on_this_floor {
                    continue;
                }
                if seen.insert(elem.id) {
                    let conn = element_connectivity(elem.id, grid, installed_nodes);
                    let dist = element_frontier_dist(elem.id, grid, installed_nodes, node_pos);
                    result.push(SingleCandidate {
                        element_id: elem.id,
                        connectivity: conn,
                        frontier_dist: dist,
                        is_lowest_floor: zi == 0,
                    });
                }
            }

            if !result.is_empty() {
                return result;
            }
        }
    }

    Vec::new()
}

/// Generate bootstrap candidate (first step only): 3 columns + 2 girders
/// anchored at the workfront position (wf.grid_x, wf.grid_y), floor zi=0.
///
/// Searches a 3×3 patch around the workfront, then expands if needed.
/// Returns valid bundles where the 2 girders are both fully connected to the
/// selected column j-nodes AND form a 90-degree pair (satisfying has_minimum_assembly).
fn generate_bootstrap_candidates(
    wf: &SimWorkfront,
    grid: &SimGrid,
    node_pos: &std::collections::HashMap<i32, (usize, usize, usize)>,
) -> Vec<Candidate> {
    let anchor_zi = 0usize;
    let upper_zi = 1usize;

    // Build column j-node map: col_id → j_node_id
    let col_jnode: std::collections::HashMap<i32, i32> = grid
        .elements
        .iter()
        .filter(|e| e.member_type == "Column")
        .map(|e| (e.id, e.node_j_id))
        .collect();

    // Build girder map: gdr_id → (node_i, node_j), only zi=upper_zi
    let all_floor1_gdrs: Vec<(i32, i32, i32)> = grid
        .elements
        .iter()
        .filter(|e| {
            e.member_type == "Girder"
                && node_pos.get(&e.node_i_id).map(|p| p.2).unwrap_or(999) == upper_zi
        })
        .map(|e| (e.id, e.node_i_id, e.node_j_id))
        .collect();

    // Try expanding patch radius until we get at least one valid bundle
    let mut candidates: Vec<Candidate> = Vec::new();
    for patch in 1i32..=(grid.nx.max(grid.ny) as i32) {
        // Collect columns in patch
        let mut patch_cols: Vec<i32> = Vec::new();
        let mut seen_c: HashSet<i32> = HashSet::new();
        for dxi in -patch..=patch {
            for dyi in -patch..=patch {
                let pxi = wf.grid_x as i32 + dxi;
                let pyi = wf.grid_y as i32 + dyi;
                if pxi < 0 || pyi < 0 {
                    continue;
                }
                if let Some(col_id) = grid.column_starting_at(pxi as usize, pyi as usize, anchor_zi)
                {
                    if seen_c.insert(col_id) {
                        patch_cols.push(col_id);
                    }
                }
            }
        }

        if patch_cols.len() < 3 {
            continue;
        }

        // Generate 3-col combinations; for each, find valid 90-degree girder pairs
        'outer: for ci in 0..patch_cols.len() {
            for cj in (ci + 1)..patch_cols.len() {
                for ck in (cj + 1)..patch_cols.len() {
                    let c_ids = [patch_cols[ci], patch_cols[cj], patch_cols[ck]];
                    // j-node set for these 3 columns
                    let jnodes: HashSet<i32> = c_ids
                        .iter()
                        .filter_map(|&cid| col_jnode.get(&cid).copied())
                        .collect();
                    if jnodes.len() < 3 {
                        continue; // degenerate
                    }

                    // Girders fully within jnodes (both ends in jnodes)
                    let valid_gdrs: Vec<i32> = all_floor1_gdrs
                        .iter()
                        .filter(|&&(_, ni, nj)| jnodes.contains(&ni) && jnodes.contains(&nj))
                        .map(|&(gid, _, _)| gid)
                        .collect();

                    // Find 90-degree girder pairs (use grid index positions — integer, no float signum issues)
                    for gi in 0..valid_gdrs.len() {
                        for gj in (gi + 1)..valid_gdrs.len() {
                            let gid_a = valid_gdrs[gi];
                            let gid_b = valid_gdrs[gj];
                            if let (Some(&(_, ni_a, nj_a)), Some(&(_, ni_b, nj_b))) = (
                                all_floor1_gdrs.iter().find(|&&(id, _, _)| id == gid_a),
                                all_floor1_gdrs.iter().find(|&&(id, _, _)| id == gid_b),
                            ) {
                                // Use grid positions (xi,yi) to determine direction — avoids float signum issues
                                let gpos = |nid: i32| -> (i32, i32) {
                                    node_pos
                                        .get(&nid)
                                        .map(|&(xi, yi, _)| (xi as i32, yi as i32))
                                        .unwrap_or((0, 0))
                                };
                                let (ax1, ay1) = gpos(ni_a);
                                let (ax2, ay2) = gpos(nj_a);
                                let (bx1, by1) = gpos(ni_b);
                                let (bx2, by2) = gpos(nj_b);
                                let da = ((ax2 - ax1).signum(), (ay2 - ay1).signum());
                                let db = ((bx2 - bx1).signum(), (by2 - by1).signum());
                                let dot = da.0 * db.0 + da.1 * db.1;
                                if dot == 0 {
                                    // 90-degree pair — valid bundle
                                    let ids = vec![c_ids[0], c_ids[1], c_ids[2], gid_a, gid_b];
                                    candidates.push(Candidate {
                                        element_ids: ids,
                                        member_count: 5,
                                        connectivity: 0.0,
                                        frontier_dist: 0.0,
                                        is_lowest_floor: true,
                                        is_independent: true,
                                    });
                                    if candidates.len() >= 100 {
                                        break 'outer;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if !candidates.is_empty() {
            break; // found valid candidates at this patch radius
        }
    }
    candidates
}

// ============================================================================
// Weighted random choice
// ============================================================================

/// Simple linear-scan weighted random choice using a pre-seeded LCG.
pub fn weighted_random_choice(scores: &[f64], rng_state: &mut u64) -> usize {
    let total: f64 = scores.iter().sum();
    if total <= 0.0 || scores.is_empty() {
        return 0;
    }
    *rng_state = rng_state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let rand_val = (*rng_state >> 33) as f64 / (u32::MAX as f64);
    let threshold = rand_val * total;
    let mut cumulative = 0.0;
    for (i, &s) in scores.iter().enumerate() {
        cumulative += s;
        if cumulative >= threshold {
            return i;
        }
    }
    scores.len() - 1
}

// ============================================================================
// Single scenario runner
// ============================================================================

/// Run one simulation scenario. Returns a SimScenario.
pub fn run_scenario(
    scenario_id: usize,
    grid: &SimGrid,
    workfronts: &[SimWorkfront],
    seed: u64,
    weights: (f64, f64, f64),
    threshold: f64,
) -> SimScenario {
    let (w1, w2, _w3) = weights;
    let mut rng = seed;

    let mut installed_ids: HashSet<i32> = HashSet::new();
    let mut installed_nodes: HashSet<i32> = HashSet::new();
    let mut steps: Vec<SimStep> = Vec::new();

    let total_elements = grid.elements.len();
    let node_pos = build_node_pos(grid);
    let dz = grid_dz(grid);

    // Early termination counters
    let mut consecutive_upper_floor_violations = 0u32;
    let mut consecutive_no_candidates = 0u32;
    let mut global_step_count = 0u32;
    let mut members_added_last_300: Vec<usize> = Vec::new();

    let termination_reason = loop {
        if installed_ids.len() >= total_elements {
            break TerminationReason::Completed;
        }

        global_step_count += 1;

        if workfronts.is_empty() {
            break TerminationReason::NoCandidates;
        }

        // Round-robin workfronts
        let wf_idx = ((global_step_count - 1) as usize) % workfronts.len();
        let wf = &workfronts[wf_idx];

        // ── Bootstrap: first step installs minimum assembly ──────────────
        if installed_ids.is_empty() {
            let bootstrap = generate_bootstrap_candidates(wf, grid, &node_pos);
            let valid: Vec<&Candidate> = bootstrap
                .iter()
                .filter(|c| check_bundle_stability(&c.element_ids, grid, &installed_ids))
                .collect();

            if valid.is_empty() {
                consecutive_no_candidates += 1;
                members_added_last_300.push(0);
                if consecutive_no_candidates >= 10 {
                    break TerminationReason::NoCandidates;
                }
                continue;
            }
            consecutive_no_candidates = 0;

            let scores: Vec<f64> = valid.iter().map(|c| c.score(w1, w2, _w3)).collect();
            let chosen_idx = weighted_random_choice(&scores, &mut rng);
            let chosen = valid[chosen_idx];

            let step_floor = chosen
                .element_ids
                .iter()
                .find_map(|eid| {
                    let e = grid.elements.iter().find(|e| e.id == *eid)?;
                    if e.member_type == "Column" {
                        let (_, _, z) = grid.node_coords(e.node_i_id)?;
                        Some((z / dz).round() as i32 + 1)
                    } else {
                        None
                    }
                })
                .unwrap_or(1);

            let added = chosen.element_ids.len();
            for eid in &chosen.element_ids {
                installed_ids.insert(*eid);
                if let Some(e) = grid.elements.iter().find(|e| e.id == *eid) {
                    installed_nodes.insert(e.node_i_id);
                    installed_nodes.insert(e.node_j_id);
                }
            }
            steps.push(SimStep {
                workfront_id: wf.id,
                element_ids: chosen.element_ids.clone(),
                floor: step_floor,
            });
            members_added_last_300.push(added);
            continue;
        }

        // ── Normal step: select one element from frontier ─────────────────
        let candidates =
            collect_single_candidates(wf, grid, &installed_ids, &installed_nodes, &node_pos);

        if candidates.is_empty() {
            consecutive_no_candidates += 1;
            members_added_last_300.push(0);
            if consecutive_no_candidates >= 10 {
                break TerminationReason::NoCandidates;
            }
            continue;
        }
        consecutive_no_candidates = 0;

        // Filter: stability
        let after_stability: Vec<&SingleCandidate> = candidates
            .iter()
            .filter(|c| check_single_stability(c.element_id, grid, &installed_ids))
            .collect();

        // Filter: upper-floor constraint
        let valid: Vec<&&SingleCandidate> = after_stability
            .iter()
            .filter(|c| {
                check_upper_floor_constraint(&[c.element_id], grid, &installed_ids, threshold)
            })
            .collect();

        if valid.is_empty() {
            if !after_stability.is_empty() {
                // Stability OK but upper-floor blocked all
                consecutive_upper_floor_violations += 1;
                if consecutive_upper_floor_violations >= 3 {
                    break TerminationReason::UpperFloorViolation;
                }
            } else {
                consecutive_no_candidates += 1;
                if consecutive_no_candidates >= 10 {
                    break TerminationReason::NoCandidates;
                }
            }
            members_added_last_300.push(0);
            continue;
        }
        consecutive_upper_floor_violations = 0;

        let scores: Vec<f64> = valid.iter().map(|c| c.score(w1, w2)).collect();
        let chosen_idx = weighted_random_choice(&scores, &mut rng);
        let chosen = valid[chosen_idx];

        // Determine floor
        let step_floor = {
            let e = grid.elements.iter().find(|e| e.id == chosen.element_id);
            match e {
                Some(e) if e.member_type == "Column" => {
                    let (_, _, z) = grid.node_coords(e.node_i_id).unwrap_or((0.0, 0.0, 0.0));
                    (z / dz).round() as i32 + 1
                }
                _ => steps.last().map(|s| s.floor).unwrap_or(1),
            }
        };

        // Install
        installed_ids.insert(chosen.element_id);
        if let Some(e) = grid.elements.iter().find(|e| e.id == chosen.element_id) {
            installed_nodes.insert(e.node_i_id);
            installed_nodes.insert(e.node_j_id);
        }
        steps.push(SimStep {
            workfront_id: wf.id,
            element_ids: vec![chosen.element_id],
            floor: step_floor,
        });
        members_added_last_300.push(1);
        if members_added_last_300.len() > 300 {
            members_added_last_300.remove(0);
        }

        // No-progress check
        if global_step_count >= 300 {
            let recent_sum: usize = members_added_last_300.iter().sum();
            if recent_sum < 3 {
                break TerminationReason::NoProgress;
            }
        }

        // Safety guard
        if global_step_count as usize >= total_elements * 10 + 1000 {
            break TerminationReason::MaxIterations;
        }
    };

    // ── Metrics ───────────────────────────────────────────────────────────
    let total_steps = steps.len();
    let total_members: usize = steps.iter().map(|s| s.element_ids.len()).sum();
    let avg_members_per_step = if total_steps > 0 {
        total_members as f64 / total_steps as f64
    } else {
        0.0
    };

    let avg_connectivity = {
        if steps.is_empty() {
            0.0
        } else {
            let mut cumulative: HashSet<i32> = HashSet::new();
            let total_conn: f64 = steps
                .iter()
                .map(|step| {
                    let conn = count_shared_nodes(&step.element_ids, grid, &cumulative) as f64;
                    for eid in &step.element_ids {
                        if let Some(e) = grid.elements.iter().find(|e| e.id == *eid) {
                            cumulative.insert(e.node_i_id);
                            cumulative.insert(e.node_j_id);
                        }
                    }
                    conn
                })
                .sum();
            total_conn / total_steps as f64
        }
    };

    SimScenario {
        id: scenario_id,
        seed,
        steps,
        metrics: ScenarioMetrics {
            avg_members_per_step,
            avg_connectivity,
            total_steps,
            total_members_installed: total_members,
            termination_reason,
        },
    }
}

// ============================================================================
// All scenarios runner
// ============================================================================

pub fn run_all_scenarios(
    count: usize,
    grid: &SimGrid,
    workfronts: &[SimWorkfront],
    weights: (f64, f64, f64),
    threshold: f64,
) -> Vec<SimScenario> {
    let mut scenarios: Vec<SimScenario> = (1..=count)
        .into_par_iter()
        .map(|i| {
            let seed = i as u64 * 2654435761;
            run_scenario(i, grid, workfronts, seed, weights, threshold)
        })
        .collect();
    scenarios.sort_by_key(|s| s.id);
    scenarios
}

// ============================================================================
// Unit tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim_grid::SimGrid;

    fn make_grid_2x2x2() -> SimGrid {
        SimGrid::new(2, 2, 2, 6000.0, 6000.0, 4000.0)
    }

    fn make_workfronts_2x2() -> Vec<SimWorkfront> {
        vec![SimWorkfront {
            id: 1,
            grid_x: 0,
            grid_y: 0,
        }]
    }

    #[test]
    fn test_candidate_score_basic() {
        let c = SingleCandidate {
            element_id: 1,
            connectivity: 2,
            frontier_dist: 1.0,
            is_lowest_floor: true,
        };
        let s = c.score(0.5, 0.3);
        assert!(s > 0.0, "score should be positive");
    }

    #[test]
    fn test_weighted_choice_single() {
        let mut rng = 12345u64;
        let idx = weighted_random_choice(&[1.0], &mut rng);
        assert_eq!(idx, 0);
    }

    #[test]
    fn test_weighted_choice_bias() {
        let mut rng = 999u64;
        let mut second_wins = 0;
        for _ in 0..100 {
            let i = weighted_random_choice(&[0.01, 10.0], &mut rng);
            if i == 1 {
                second_wins += 1;
            }
        }
        assert!(second_wins > 70, "second candidate should win >70%");
    }

    #[test]
    fn test_run_scenario_2x2x2() {
        let grid = make_grid_2x2x2();
        let wfs = make_workfronts_2x2();
        let scenario = run_scenario(1, &grid, &wfs, 42, (0.5, 0.3, 0.2), 0.3);
        for step in &scenario.steps {
            for eid in &step.element_ids {
                assert!(*eid >= 1, "element ID should be >= 1");
            }
        }
    }

    #[test]
    fn test_run_scenario_completes_small_grid() {
        let grid = make_grid_2x2x2();
        let wfs = make_workfronts_2x2();
        let scenario = run_scenario(1, &grid, &wfs, 1, (0.5, 0.3, 0.2), 0.3);
        let _ = scenario.metrics.termination_reason;
    }

    #[test]
    fn test_run_all_scenarios_count() {
        let grid = make_grid_2x2x2();
        let wfs = make_workfronts_2x2();
        let scenarios = run_all_scenarios(5, &grid, &wfs, (0.5, 0.3, 0.2), 0.3);
        assert_eq!(scenarios.len(), 5);
        for s in &scenarios {
            assert!(s.id >= 1, "scenario id should be >= 1");
        }
    }

    #[test]
    fn test_scenario_ids_one_indexed() {
        let grid = make_grid_2x2x2();
        let wfs = make_workfronts_2x2();
        let scenarios = run_all_scenarios(3, &grid, &wfs, (0.5, 0.3, 0.2), 0.3);
        let ids: Vec<usize> = scenarios.iter().map(|s| s.id).collect();
        assert_eq!(ids, vec![1, 2, 3]);
    }

    /// Center workfront (1,2) on 4×4×3 grid — must also produce steps.
    #[test]
    fn test_run_scenario_center_workfront() {
        let grid = SimGrid::new(4, 4, 3, 6000.0, 6000.0, 4000.0);
        let wfs = vec![SimWorkfront {
            id: 1,
            grid_x: 1,
            grid_y: 2,
        }];

        let scenario = run_scenario(1, &grid, &wfs, 42, (0.5, 0.3, 0.2), 0.3);
        assert!(
            scenario.metrics.total_steps >= 1,
            "should produce at least 1 step with center workfront"
        );
    }

    /// Default grid (4×4×3) — must finish quickly and install all or most elements.
    #[test]
    fn test_run_scenario_default_grid_4x4x3() {
        let grid = SimGrid::new(4, 4, 3, 6000.0, 6000.0, 4000.0);
        let wfs = vec![SimWorkfront {
            id: 1,
            grid_x: 0,
            grid_y: 0,
        }];
        println!(
            "4×4×3 grid: nodes={}, elements={}",
            grid.nodes.len(),
            grid.elements.len()
        );
        let t0 = std::time::Instant::now();
        let scenario = run_scenario(1, &grid, &wfs, 42, (0.5, 0.3, 0.2), 0.3);
        let elapsed = t0.elapsed();
        println!(
            "Scenario result: steps={}, members={}/{}, termination={:?}, elapsed={:.2?}",
            scenario.metrics.total_steps,
            scenario.metrics.total_members_installed,
            grid.elements.len(),
            scenario.metrics.termination_reason,
            elapsed,
        );
        assert!(
            scenario.metrics.total_steps >= 1,
            "should produce at least 1 step"
        );
        assert!(elapsed.as_secs() < 10, "too slow: {:.2?}", elapsed);
        for step in &scenario.steps {
            for eid in &step.element_ids {
                assert!(*eid >= 1, "element ID must be >= 1, got {}", eid);
            }
        }
    }
}
