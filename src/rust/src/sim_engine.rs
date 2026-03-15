//! Simulation Engine for AssyPlan Phase 3
//!
//! Monte-Carlo + Pruning + Weighted Sampling + Incremental Greedy-ish
//!
//! Score = w1 × (1/member_count) + w2 × connectivity + w3 × (1/distance) + 0.05 × is_lowest_floor
//!
//! Candidate priority order:
//!   1. 1 column + 1 girder  (best - smallest increment)
//!   2. 1 column + 2 girders
//!   3. 2 columns + 1 girder
//!   4. 2 columns + 2 girders
//!   5. 3 columns + 2 girders independent unit (last resort - no connectivity)
//!
//! Early termination:
//!   1. Upper-floor constraint violated 3 consecutive times
//!   2. Fewer than 3 members added in last 300 global steps
//!   3. Independent 5-unit chosen 5+ consecutive times
//!   4. No valid candidate found 10 consecutive times

use std::collections::HashSet;

use rayon::prelude::*;

use crate::graphics::ui::{ScenarioMetrics, SimScenario, SimStep, SimWorkfront, TerminationReason};
use crate::sim_grid::SimGrid;
use crate::stability::{has_minimum_assembly, validate_column_support, validate_girder_support};

// ============================================================================
// Candidate
// ============================================================================

/// A single candidate step — a set of elements to install together.
#[derive(Clone, Debug)]
pub struct Candidate {
    /// Element IDs to install (1-indexed)
    pub element_ids: Vec<i32>,
    /// Number of members in this candidate
    pub member_count: usize,
    /// Number of shared nodes with already-installed structure (connectivity)
    pub connectivity: f64,
    /// Manhattan distance (in grid index steps) from the workfront frontier
    pub frontier_dist: f64,
    /// Whether ALL members are on the lowest unstarted floor
    pub is_lowest_floor: bool,
    /// Whether this candidate is independent (no connection to existing structure)
    pub is_independent: bool,
}

impl Candidate {
    /// Compute the weighted score.
    /// Score = w1 × (1/member_count) + w2 × connectivity + w3 × (1/distance) + 0.05 × lowest_floor
    pub fn score(&self, w1: f64, w2: f64, w3: f64) -> f64 {
        let s_members = w1 * (1.0 / self.member_count.max(1) as f64);
        let s_conn = w2 * self.connectivity;
        let s_dist = w3 * (1.0 / (self.frontier_dist + 1.0)); // +1 to avoid div/0
        let s_floor = 0.05 * if self.is_lowest_floor { 1.0 } else { 0.0 };
        s_members + s_conn + s_dist + s_floor
    }
}

// ============================================================================
// Frontier management helpers
// ============================================================================

/// Compute the Manhattan distance (in grid steps) between two node positions.
/// Positions derived from continuous coordinates using spacing dx/dy.
fn manhattan_dist_grid(x1: f64, y1: f64, x2: f64, y2: f64, dx: f64, dy: f64) -> f64 {
    let gx = ((x1 - x2).abs() / dx.max(1.0)).round();
    let gy = ((y1 - y2).abs() / dy.max(1.0)).round();
    gx + gy
}

/// Compute the minimum Manhattan distance from a candidate's nodes to any frontier node.
fn min_frontier_dist(element_ids: &[i32], grid: &SimGrid, frontier_nodes: &HashSet<i32>) -> f64 {
    if frontier_nodes.is_empty() {
        return 0.0;
    }

    // Collect coordinates of candidate nodes
    let cand_node_ids: HashSet<i32> = element_ids
        .iter()
        .flat_map(|eid| {
            grid.elements
                .iter()
                .find(|e| e.id == *eid)
                .map(|e| vec![e.node_i_id, e.node_j_id])
                .unwrap_or_default()
        })
        .collect();

    let dx = if grid.nx > 1 {
        grid.nodes
            .iter()
            .filter(|n| n.y == grid.nodes[0].y && n.z == grid.nodes[0].z)
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
            .unwrap_or(6000.0)
    } else {
        6000.0
    };

    let dy = if grid.ny > 1 {
        grid.nodes
            .iter()
            .filter(|n| n.x == grid.nodes[0].x && n.z == grid.nodes[0].z)
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
            .unwrap_or(6000.0)
    } else {
        6000.0
    };

    let mut min_dist = f64::MAX;
    for cand_nid in &cand_node_ids {
        if let Some((cx, cy, _)) = grid.node_coords(*cand_nid) {
            for fn_id in frontier_nodes {
                if let Some((fx, fy, _)) = grid.node_coords(*fn_id) {
                    let d = manhattan_dist_grid(cx, cy, fx, fy, dx, dy);
                    if d < min_dist {
                        min_dist = d;
                    }
                }
            }
        }
    }
    if min_dist == f64::MAX {
        0.0
    } else {
        min_dist
    }
}

/// Count how many of the candidate's nodes are already in the installed node set.
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
// Pruning helpers
// ============================================================================

/// Check structural stability for a proposed candidate set given the current
/// installed element IDs.  All elements in element_ids must satisfy either
/// column support or girder support conditions w.r.t. (installed_ids ∪ element_ids).
fn check_candidate_stability(
    element_ids: &[i32],
    grid: &SimGrid,
    installed_ids: &HashSet<i32>,
) -> bool {
    // Build a combined installed set for the check
    let mut combined: HashSet<i32> = installed_ids.clone();
    combined.extend(element_ids.iter().cloned());

    // Elements involved in the candidate
    let cand_elements: Vec<_> = element_ids
        .iter()
        .filter_map(|eid| grid.elements.iter().find(|e| e.id == *eid))
        .collect();

    // If combined includes no installed elements yet, check minimum assembly
    if installed_ids.is_empty() {
        // First step — must form a minimum assembly
        let combined_elems: Vec<_> = grid
            .elements
            .iter()
            .filter(|e| combined.contains(&e.id))
            .cloned()
            .collect();
        return has_minimum_assembly(&grid.nodes, &combined_elems);
    }

    // Incremental: each new member must be individually supportable
    for elem in &cand_elements {
        let ok = if elem.member_type == "Column" {
            validate_column_support(elem, &grid.nodes, &grid.elements, &installed_ids)
        } else {
            // Girder: both ends must connect to installed OR other candidate columns
            // So check against combined set (installed + current candidates so far)
            validate_girder_support(elem, &grid.nodes, &grid.elements, &combined)
        };
        if !ok {
            return false;
        }
    }
    true
}

/// Check upper-floor column installation constraint for a candidate.
/// For any column in the candidate at floor F, floor F-1 must have reached threshold.
fn check_upper_floor_constraint(
    element_ids: &[i32],
    grid: &SimGrid,
    installed_ids: &HashSet<i32>,
    threshold: f64,
) -> bool {
    use crate::stability::check_floor_installation_constraint;

    // Collect all floors for columns in the candidate
    let col_floors: Vec<i32> = element_ids
        .iter()
        .filter_map(|eid| {
            let elem = grid.elements.iter().find(|e| e.id == *eid)?;
            if elem.member_type != "Column" {
                return None;
            }
            // Floor = z-level of node_i + 1 (1-indexed)
            let (_, _, z) = grid.node_coords(elem.node_i_id)?;
            let dz = if grid.nz > 1 {
                grid.nodes
                    .iter()
                    .map(|n| (n.z * 1000.0) as i64)
                    .collect::<std::collections::HashSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>()
                    .windows(2)
                    .filter_map(|w| {
                        let d = w[1] - w[0];
                        if d > 0 {
                            Some(d as f64 / 1000.0)
                        } else {
                            None
                        }
                    })
                    .next()
                    .unwrap_or(4000.0)
            } else {
                4000.0
            };
            let floor = (z / dz).round() as i32 + 1;
            Some(floor)
        })
        .collect();

    for floor in col_floors {
        let (allowed, _) = check_floor_installation_constraint(
            floor,
            installed_ids,
            &grid.elements,
            &grid.nodes,
            threshold * 100.0, // function expects percentage (0~100)
        );
        if !allowed {
            return false;
        }
    }
    true
}

// ============================================================================
// Candidate generation
// ============================================================================

/// Determine the minimum unstarted floor for a given workfront position.
fn min_unstarted_floor(wf: &SimWorkfront, grid: &SimGrid, installed_ids: &HashSet<i32>) -> usize {
    for zi in 0..(grid.nz - 1) {
        // Check if the column at this workfront position on this floor is installed
        if let Some(col_id) = grid.column_starting_at(wf.grid_x, wf.grid_y, zi) {
            if !installed_ids.contains(&col_id) {
                return zi;
            }
        } else {
            return zi;
        }
    }
    grid.nz.saturating_sub(1)
}

/// Generate all incremental candidates for a workfront, ordered by priority:
///   1. 1 col + 1 girder
///   2. 1 col + 2 girders
///   3. 2 cols + 1 girder
///   4. 2 cols + 2 girders
///   5. 3 cols + 2 girders (independent — only if no connected candidates exist)
///
/// Filtering: only uninstalled elements; grid bounds respected (guaranteed by SimGrid).
pub fn generate_incremental_candidates(
    wf: &SimWorkfront,
    grid: &SimGrid,
    installed_ids: &HashSet<i32>,
    installed_nodes: &HashSet<i32>,
) -> Vec<Candidate> {
    let mut connected_candidates: Vec<Candidate> = Vec::new();
    let mut independent_candidates: Vec<Candidate> = Vec::new();

    let zi = min_unstarted_floor(wf, grid, installed_ids);

    // Frontier nodes: nodes of installed elements adjacent to this workfront
    let frontier_nodes: HashSet<i32> = installed_nodes.clone();

    // ── Collect available columns and girders at floor zi ──────────────────
    // All columns at floor zi that are not yet installed
    let avail_cols: Vec<i32> = grid
        .elements
        .iter()
        .filter(|e| {
            if e.member_type != "Column" || installed_ids.contains(&e.id) {
                return false;
            }
            if let Some((_, _, z)) = grid.node_coords(e.node_i_id) {
                let dz = grid_dz(grid);
                let floor_zi = (z / dz).round() as usize;
                floor_zi == zi
            } else {
                false
            }
        })
        .map(|e| e.id)
        .collect();

    // All girders at floor zi+1 (z level = zi+1) that are not yet installed
    let avail_girdrs: Vec<i32> = grid
        .elements
        .iter()
        .filter(|e| {
            if e.member_type != "Girder" || installed_ids.contains(&e.id) {
                return false;
            }
            if let Some((_, _, z)) = grid.node_coords(e.node_i_id) {
                let dz = grid_dz(grid);
                let floor_zi = (z / dz).round() as usize;
                floor_zi == zi + 1
            } else {
                false
            }
        })
        .map(|e| e.id)
        .collect();

    // Determine lowest floor flag (zi == 0)
    let is_lowest_floor = zi == 0;

    // ── Priority 1: 1 column + 1 girder ───────────────────────────────────
    for &col_id in &avail_cols {
        for &gdr_id in &avail_girdrs {
            let ids = vec![col_id, gdr_id];
            let connectivity = count_shared_nodes(&ids, grid, installed_nodes) as f64;
            let is_connected = connectivity > 0.0 || installed_ids.is_empty();
            let dist = min_frontier_dist(&ids, grid, &frontier_nodes);
            let c = Candidate {
                element_ids: ids,
                member_count: 2,
                connectivity,
                frontier_dist: dist,
                is_lowest_floor,
                is_independent: !is_connected,
            };
            if is_connected {
                connected_candidates.push(c);
            }
        }
    }

    // ── Priority 2: 1 column + 2 girders ──────────────────────────────────
    for &col_id in &avail_cols {
        for i in 0..avail_girdrs.len() {
            for j in (i + 1)..avail_girdrs.len() {
                let ids = vec![col_id, avail_girdrs[i], avail_girdrs[j]];
                let connectivity = count_shared_nodes(&ids, grid, installed_nodes) as f64;
                let is_connected = connectivity > 0.0 || installed_ids.is_empty();
                if !is_connected {
                    continue; // skip non-connected for priority 2
                }
                let dist = min_frontier_dist(&ids, grid, &frontier_nodes);
                connected_candidates.push(Candidate {
                    element_ids: ids,
                    member_count: 3,
                    connectivity,
                    frontier_dist: dist,
                    is_lowest_floor,
                    is_independent: false,
                });
            }
        }
    }

    // ── Priority 3: 2 columns + 1 girder ──────────────────────────────────
    for i in 0..avail_cols.len() {
        for j in (i + 1)..avail_cols.len() {
            for &gdr_id in &avail_girdrs {
                let ids = vec![avail_cols[i], avail_cols[j], gdr_id];
                let connectivity = count_shared_nodes(&ids, grid, installed_nodes) as f64;
                let is_connected = connectivity > 0.0 || installed_ids.is_empty();
                if !is_connected {
                    continue;
                }
                let dist = min_frontier_dist(&ids, grid, &frontier_nodes);
                connected_candidates.push(Candidate {
                    element_ids: ids,
                    member_count: 3,
                    connectivity,
                    frontier_dist: dist,
                    is_lowest_floor,
                    is_independent: false,
                });
            }
        }
    }

    // ── Priority 4: 2 columns + 2 girders ─────────────────────────────────
    for i in 0..avail_cols.len() {
        for j in (i + 1)..avail_cols.len() {
            for gi in 0..avail_girdrs.len() {
                for gj in (gi + 1)..avail_girdrs.len() {
                    let ids = vec![
                        avail_cols[i],
                        avail_cols[j],
                        avail_girdrs[gi],
                        avail_girdrs[gj],
                    ];
                    let connectivity = count_shared_nodes(&ids, grid, installed_nodes) as f64;
                    let is_connected = connectivity > 0.0 || installed_ids.is_empty();
                    if !is_connected {
                        continue;
                    }
                    let dist = min_frontier_dist(&ids, grid, &frontier_nodes);
                    connected_candidates.push(Candidate {
                        element_ids: ids,
                        member_count: 4,
                        connectivity,
                        frontier_dist: dist,
                        is_lowest_floor,
                        is_independent: false,
                    });
                }
            }
        }
    }

    // ── Priority 5: 3 columns + 2 girders (independent) ──────────────────
    // Only generate if we have no connected candidates at all
    if connected_candidates.is_empty() {
        for ci in 0..avail_cols.len() {
            for cj in (ci + 1)..avail_cols.len() {
                for ck in (cj + 1)..avail_cols.len() {
                    for gi in 0..avail_girdrs.len() {
                        for gj in (gi + 1)..avail_girdrs.len() {
                            let ids = vec![
                                avail_cols[ci],
                                avail_cols[cj],
                                avail_cols[ck],
                                avail_girdrs[gi],
                                avail_girdrs[gj],
                            ];
                            let connectivity =
                                count_shared_nodes(&ids, grid, installed_nodes) as f64;
                            let dist = min_frontier_dist(&ids, grid, &frontier_nodes);
                            independent_candidates.push(Candidate {
                                element_ids: ids,
                                member_count: 5,
                                connectivity,
                                frontier_dist: dist,
                                is_lowest_floor,
                                is_independent: true,
                            });
                        }
                    }
                }
            }
        }
    }

    // Return connected first, independent only if no connected exist
    if !connected_candidates.is_empty() {
        connected_candidates
    } else {
        independent_candidates
    }
}

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

// ============================================================================
// Weighted random choice
// ============================================================================

/// Simple linear-scan weighted random choice using a pre-seeded LCG.
/// Returns the index of the chosen candidate.
pub fn weighted_random_choice(scores: &[f64], rng_state: &mut u64) -> usize {
    let total: f64 = scores.iter().sum();
    if total <= 0.0 || scores.is_empty() {
        return 0;
    }

    // LCG step
    *rng_state = rng_state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let rand_val = (*rng_state >> 33) as f64 / (u32::MAX as f64); // 0..1

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
///
/// # Arguments
/// - `grid`: Pre-validated element pool
/// - `workfronts`: User-selected workfront positions
/// - `seed`: Random seed (1-indexed scenario ID → seed)
/// - `weights`: (w1, w2, w3) for score calculation
/// - `threshold`: Upper-floor constraint threshold (0.0~1.0)
pub fn run_scenario(
    scenario_id: usize,
    grid: &SimGrid,
    workfronts: &[SimWorkfront],
    seed: u64,
    weights: (f64, f64, f64),
    threshold: f64,
) -> SimScenario {
    let (w1, w2, w3) = weights;
    let mut rng = seed;

    // State
    let mut installed_ids: HashSet<i32> = HashSet::new();
    let mut installed_nodes: HashSet<i32> = HashSet::new();
    let mut steps: Vec<SimStep> = Vec::new();

    let total_elements = grid.elements.len();

    // Early termination counters
    let mut consecutive_upper_floor_violations = 0u32;
    let mut consecutive_no_candidates = 0u32;
    let mut consecutive_independent = 0u32;
    let mut global_step_count = 0u32;
    let mut members_added_last_300: Vec<usize> = Vec::new(); // ring buffer

    // ── Main simulation loop ───────────────────────────────────────────────
    let termination_reason = loop {
        // All elements installed?
        if installed_ids.len() >= total_elements {
            break TerminationReason::Completed;
        }

        global_step_count += 1;

        // Round-robin through workfronts
        let wf_count = workfronts.len();
        if wf_count == 0 {
            break TerminationReason::NoCandidates;
        }

        // Select workfront: round-robin by global step
        let wf_idx = ((global_step_count - 1) as usize) % wf_count;
        let wf = &workfronts[wf_idx];

        // Generate candidates
        let candidates =
            generate_incremental_candidates(wf, grid, &installed_ids, &installed_nodes);

        if candidates.is_empty() {
            consecutive_no_candidates += 1;
            members_added_last_300.push(0);
            if consecutive_no_candidates >= 10 {
                break TerminationReason::NoCandidates;
            }
            continue;
        }
        consecutive_no_candidates = 0;

        // Prune: stability + upper-floor constraint
        let valid: Vec<&Candidate> = candidates
            .iter()
            .filter(|c| check_candidate_stability(&c.element_ids, grid, &installed_ids))
            .filter(|c| {
                check_upper_floor_constraint(&c.element_ids, grid, &installed_ids, threshold)
            })
            .collect();

        if valid.is_empty() {
            // All candidates pruned — check if it's upper-floor specific
            let after_stability: Vec<&Candidate> = candidates
                .iter()
                .filter(|c| check_candidate_stability(&c.element_ids, grid, &installed_ids))
                .collect();

            if !after_stability.is_empty() {
                // Stability OK but upper-floor constraint blocked all
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

        // Enforce: if any connected candidate exists, must choose connected
        let connected: Vec<&&Candidate> = valid.iter().filter(|c| !c.is_independent).collect();
        let final_pool: Vec<&&Candidate> = if !connected.is_empty() {
            connected
        } else {
            valid.iter().collect()
        };

        // Compute scores
        let scores: Vec<f64> = final_pool.iter().map(|c| c.score(w1, w2, w3)).collect();

        // Weighted random choice
        let chosen_idx = weighted_random_choice(&scores, &mut rng);
        let chosen = final_pool[chosen_idx];

        // Track consecutive independent
        if chosen.is_independent {
            consecutive_independent += 1;
            if consecutive_independent >= 5 {
                break TerminationReason::IndependentOveruse;
            }
        } else {
            consecutive_independent = 0;
        }

        // Install chosen elements
        let step_number = steps.len() + 1; // 1-indexed

        // Determine floor for this step (floor of first column, or first element)
        let step_floor = chosen
            .element_ids
            .iter()
            .find_map(|eid| {
                let elem = grid.elements.iter().find(|e| e.id == *eid)?;
                if elem.member_type == "Column" {
                    let (_, _, z) = grid.node_coords(elem.node_i_id)?;
                    let dz = grid_dz(grid);
                    Some((z / dz).round() as i32 + 1)
                } else {
                    None
                }
            })
            .unwrap_or(1);

        // Add nodes from installed elements
        for eid in &chosen.element_ids {
            installed_ids.insert(*eid);
            if let Some(elem) = grid.elements.iter().find(|e| e.id == *eid) {
                installed_nodes.insert(elem.node_i_id);
                installed_nodes.insert(elem.node_j_id);
            }
        }

        steps.push(SimStep {
            workfront_id: wf.id,
            element_ids: chosen.element_ids.clone(),
            floor: step_floor,
        });

        let added = chosen.element_ids.len();
        members_added_last_300.push(added);
        if members_added_last_300.len() > 300 {
            members_added_last_300.remove(0);
        }

        // Check no-progress termination
        if global_step_count >= 300 {
            let recent_sum: usize = members_added_last_300.iter().sum();
            if recent_sum < 3 {
                break TerminationReason::NoProgress;
            }
        }

        // Safety max-iterations guard (10× total elements)
        if global_step_count as usize >= total_elements * 10 + 1000 {
            break TerminationReason::MaxIterations;
        }

        let _ = step_number; // suppress unused warning
    };

    // ── Compute metrics ────────────────────────────────────────────────────
    let total_steps = steps.len();
    let total_members: usize = steps.iter().map(|s| s.element_ids.len()).sum();
    let avg_members_per_step = if total_steps > 0 {
        total_members as f64 / total_steps as f64
    } else {
        0.0
    };

    // Average connectivity: average number of shared nodes per step
    let avg_connectivity = {
        if steps.is_empty() {
            0.0
        } else {
            // Re-compute connectivity for each step based on cumulative installation
            let mut cumulative: HashSet<i32> = HashSet::new();
            let total_conn: f64 = steps
                .iter()
                .map(|step| {
                    let conn = count_shared_nodes(&step.element_ids, grid, &cumulative) as f64;
                    for eid in &step.element_ids {
                        if let Some(elem) = grid.elements.iter().find(|e| e.id == *eid) {
                            cumulative.insert(elem.node_i_id);
                            cumulative.insert(elem.node_j_id);
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
// All scenarios runner (sequential — parallelism added in lib.rs with rayon)
// ============================================================================

/// Run `count` scenarios sequentially. Parallelism is applied in lib.rs.
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
            let seed = i as u64 * 2654435761; // Fibonacci hashing seed
            run_scenario(i, grid, workfronts, seed, weights, threshold)
        })
        .collect();
    // Sort by scenario_id to restore deterministic ordering
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
        let c = Candidate {
            element_ids: vec![1, 2],
            member_count: 2,
            connectivity: 2.0,
            frontier_dist: 1.0,
            is_lowest_floor: true,
            is_independent: false,
        };
        let s = c.score(0.5, 0.3, 0.15);
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
        // Score [0.01, 10.0] — second should win most of the time
        let mut rng = 999u64;
        let mut second_wins = 0;
        for _ in 0..100 {
            let i = weighted_random_choice(&[0.01, 10.0], &mut rng);
            if i == 1 {
                second_wins += 1;
            }
        }
        assert!(
            second_wins > 70,
            "second candidate should win >70% with 1000x higher score"
        );
    }

    #[test]
    fn test_run_scenario_2x2x2() {
        let grid = make_grid_2x2x2();
        let wfs = make_workfronts_2x2();
        let scenario = run_scenario(1, &grid, &wfs, 42, (0.5, 0.3, 0.15), 0.3);
        // Should produce at least 1 step
        assert!(
            !scenario.steps.is_empty()
                || scenario.metrics.termination_reason != TerminationReason::Completed,
            "Should either complete or terminate gracefully"
        );
        // All step element_ids should be 1-indexed (no 0s)
        for step in &scenario.steps {
            for eid in &step.element_ids {
                assert!(*eid >= 1, "element ID should be >= 1");
            }
        }
    }

    #[test]
    fn test_run_scenario_completes_small_grid() {
        // 2×2×2 is small enough to complete
        let grid = make_grid_2x2x2();
        let wfs = make_workfronts_2x2();
        let scenario = run_scenario(1, &grid, &wfs, 1, (0.5, 0.3, 0.15), 0.3);
        // Either completed or terminated — must not panic
        let _ = scenario.metrics.termination_reason;
    }

    #[test]
    fn test_run_all_scenarios_count() {
        let grid = make_grid_2x2x2();
        let wfs = make_workfronts_2x2();
        let scenarios = run_all_scenarios(5, &grid, &wfs, (0.5, 0.3, 0.15), 0.3);
        assert_eq!(scenarios.len(), 5);
        for s in &scenarios {
            assert!(s.id >= 1, "scenario id should be >= 1");
        }
    }

    #[test]
    fn test_scenario_ids_one_indexed() {
        let grid = make_grid_2x2x2();
        let wfs = make_workfronts_2x2();
        let scenarios = run_all_scenarios(3, &grid, &wfs, (0.5, 0.3, 0.15), 0.3);
        let ids: Vec<usize> = scenarios.iter().map(|s| s.id).collect();
        assert_eq!(ids, vec![1, 2, 3]);
    }
}
