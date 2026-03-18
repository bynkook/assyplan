//! Simulation Engine for AssyPlan Phase 3
//!
//! Sequence and step are separate concepts:
//! - sequence: individual member installation order
//! - step: pattern-based stability unit that may contain multiple sequences

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use rayon::prelude::*;

use crate::graphics::ui::{
    LocalStep, ScenarioMetrics, SimScenario, SimSequence, SimStep, SimWorkfront,
    TerminationReason,
};
use crate::sim_grid::SimGrid;
use crate::stability::{
    check_step_bundle_stability, classify_member_signature, StepBufferDecision, StepPatternType,
    StabilityElement,
};

#[derive(Clone, Copy, Debug)]
pub struct SimConstraints {
    pub upper_floor_column_rate_threshold: f64,
    pub lower_floor_completion_ratio_threshold: f64,
    pub lower_floor_forced_completion_threshold: usize,
}

#[derive(Default, Clone)]
struct WorkfrontState {
    owned_ids: HashSet<i32>,
    buffer_sequences: Vec<SimSequence>,
    planned_pattern: Vec<i32>,
    committed_floor: Option<i32>,
    last_failed_floor: Option<i32>,
}

impl WorkfrontState {
    fn all_local_ids(&self) -> HashSet<i32> {
        let mut ids = self.owned_ids.clone();
        ids.extend(self.buffer_sequences.iter().map(|seq| seq.element_id));
        ids
    }

    fn buffer_element_ids(&self) -> Vec<i32> {
        self.buffer_sequences
            .iter()
            .map(|seq| seq.element_id)
            .collect()
    }
}

#[derive(Clone)]
struct PatternChoice {
    element_ids: Vec<i32>,
    pattern: StepPatternType,
}

#[derive(Clone)]
struct SingleCandidate {
    element_id: i32,
    connectivity: usize,
    frontier_dist: f64,
}

impl SingleCandidate {
    fn score(&self, w1: f64, w2: f64) -> f64 {
        let connectivity_score = self.connectivity as f64;
        let frontier_score = 1.0 / (1.0 + self.frontier_dist.max(0.0));
        (w1 * connectivity_score) + (w2 * frontier_score)
    }
}

#[derive(Clone)]
struct Candidate {
    element_ids: Vec<i32>,
    member_count: usize,
    connectivity: f64,
    frontier_dist: f64,
    is_lowest_floor: bool,
    is_independent: bool,
}

impl Candidate {
    fn score(&self, w1: f64, w2: f64, w3: f64) -> f64 {
        let member_score = self.member_count as f64;
        let frontier_score = 1.0 / (1.0 + self.frontier_dist.max(0.0));
        let floor_bonus = if self.is_lowest_floor { 0.2 } else { 0.0 };
        let independent_bonus = if self.is_independent { 0.1 } else { 0.0 };
        (w1 * member_score)
            + (w2 * self.connectivity)
            + (w3 * frontier_score)
            + floor_bonus
            + independent_bonus
    }
}

fn grid_dz(grid: &SimGrid) -> f64 {
    if grid.dz > 0.0 {
        return grid.dz;
    }

    let mut z_values: Vec<f64> = grid.nodes.iter().map(|n| n.z).collect();
    z_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    z_values.dedup_by(|a, b| (*a - *b).abs() < 1e-9);

    for pair in z_values.windows(2) {
        let dz = pair[1] - pair[0];
        if dz > 0.0 {
            return dz;
        }
    }

    1.0
}

struct FloorTracker {
    total_per_floor: HashMap<i32, usize>,
    column_floor_by_element: HashMap<i32, i32>,
    max_floor: i32,
}

impl FloorTracker {
    fn from_grid(grid: &SimGrid, dz: f64) -> Self {
        let mut total_per_floor: HashMap<i32, usize> = HashMap::new();
        let mut column_floor_by_element: HashMap<i32, i32> = HashMap::new();

        for elem in grid.elements.iter().filter(|e| e.member_type == "Column") {
            let floor = grid
                .element_floor_by_id
                .get(&elem.id)
                .copied()
                .or_else(|| {
                    grid.node_coords(elem.node_i_id)
                        .map(|(_, _, z)| (z / dz).round() as i32 + 1)
                });

            if let Some(floor) = floor {
                column_floor_by_element.insert(elem.id, floor);
                *total_per_floor.entry(floor).or_insert(0) += 1;
            }
        }

        let max_floor = total_per_floor.keys().copied().max().unwrap_or(1);

        Self {
            total_per_floor,
            column_floor_by_element,
            max_floor,
        }
    }

    fn installed_per_floor_from(&self, installed_ids: &HashSet<i32>) -> HashMap<i32, usize> {
        let mut installed_per_floor: HashMap<i32, usize> = HashMap::new();
        for eid in installed_ids {
            if let Some(floor) = self.column_floor_by_element.get(eid) {
                *installed_per_floor.entry(*floor).or_insert(0) += 1;
            }
        }
        installed_per_floor
    }
}

fn build_node_pos(grid: &SimGrid) -> HashMap<i32, (usize, usize, usize)> {
    grid.node_index
        .iter()
        .map(|(&pos, &id)| (id, pos))
        .collect()
}

fn get_element(grid: &SimGrid, element_id: i32) -> Option<&StabilityElement> {
    grid.element_index_by_id
        .get(&element_id)
        .and_then(|idx| grid.elements.get(*idx))
}

fn is_column(grid: &SimGrid, element_id: i32) -> bool {
    get_element(grid, element_id)
        .map(|e| e.member_type == "Column")
        .unwrap_or(false)
}

fn classify_buffer(
    buffer_element_ids: &[i32],
    grid: &SimGrid,
    has_stable_structure: bool,
) -> StepBufferDecision {
    let signature: String = buffer_element_ids
        .iter()
        .map(|eid| if is_column(grid, *eid) { 'C' } else { 'G' })
        .collect();

    classify_member_signature(signature.as_str(), has_stable_structure)
}

fn reorder_bootstrap_pattern(
    element_ids: &[i32],
    grid: &SimGrid,
    node_pos: &HashMap<i32, (usize, usize, usize)>,
    wf: &SimWorkfront,
) -> Vec<i32> {
    let columns: Vec<i32> = element_ids
        .iter()
        .copied()
        .filter(|eid| is_column(grid, *eid))
        .collect();
    let girders: Vec<i32> = element_ids
        .iter()
        .copied()
        .filter(|eid| !is_column(grid, *eid))
        .collect();

    if columns.len() != 3 || girders.len() != 2 {
        return element_ids.to_vec();
    }

    let Some(g1) = get_element(grid, girders[0]) else {
        return element_ids.to_vec();
    };
    let Some(g2) = get_element(grid, girders[1]) else {
        return element_ids.to_vec();
    };

    let shared_node = [g1.node_i_id, g1.node_j_id]
        .into_iter()
        .find(|node_id| *node_id == g2.node_i_id || *node_id == g2.node_j_id);

    let Some(shared_node) = shared_node else {
        return element_ids.to_vec();
    };

    let mut central_col: Option<i32> = None;
    let mut outer_cols: Vec<i32> = Vec::new();
    for col_id in columns {
        if let Some(col) = get_element(grid, col_id) {
            if col.node_j_id == shared_node {
                central_col = Some(col_id);
            } else {
                outer_cols.push(col_id);
            }
        }
    }

    let Some(central_col) = central_col else {
        return element_ids.to_vec();
    };
    if outer_cols.len() != 2 {
        return element_ids.to_vec();
    }

    let column_distance = |col_id: i32| -> i32 {
        get_element(grid, col_id)
            .and_then(|col| node_pos.get(&col.node_i_id))
            .map(|&(xi, yi, _)| {
                (xi as i32 - wf.grid_x as i32).abs() + (yi as i32 - wf.grid_y as i32).abs()
            })
            .unwrap_or(i32::MAX)
    };

    let mut ordered_outer = outer_cols;
    ordered_outer.sort_by_key(|col_id| column_distance(*col_id));

    let first_col = if column_distance(central_col) <= column_distance(ordered_outer[0]) {
        central_col
    } else {
        ordered_outer[0]
    };

    let remaining_outer: Vec<i32> = ordered_outer
        .iter()
        .copied()
        .filter(|col_id| *col_id != first_col)
        .collect();

    let second_col = if first_col == central_col {
        ordered_outer[0]
    } else {
        central_col
    };

    let third_col = if first_col == central_col {
        ordered_outer[1]
    } else {
        remaining_outer[0]
    };

    let girder_between = |col_a: i32, col_b: i32| -> Option<i32> {
        let node_a = get_element(grid, col_a)?.node_j_id;
        let node_b = get_element(grid, col_b)?.node_j_id;
        grid.girder_between(node_a, node_b)
    };

    let Some(first_girder) = girder_between(first_col, second_col) else {
        return element_ids.to_vec();
    };
    let Some(second_girder) = girder_between(central_col, third_col) else {
        return element_ids.to_vec();
    };

    vec![first_col, second_col, first_girder, third_col, second_girder]
}

fn other_node(element: &StabilityElement, node_id: i32) -> Option<i32> {
    if element.node_i_id == node_id {
        Some(element.node_j_id)
    } else if element.node_j_id == node_id {
        Some(element.node_i_id)
    } else {
        None
    }
}

fn node_grid_dist(nid_a: i32, nid_b: i32, node_pos: &HashMap<i32, (usize, usize, usize)>) -> f64 {
    match (node_pos.get(&nid_a), node_pos.get(&nid_b)) {
        (Some(&(ax, ay, _)), Some(&(bx, by, _))) => {
            ((ax as i32 - bx as i32).abs() + (ay as i32 - by as i32).abs()) as f64
        }
        _ => f64::MAX,
    }
}

fn element_frontier_dist(
    element_id: i32,
    grid: &SimGrid,
    installed_nodes: &HashSet<i32>,
    node_pos: &HashMap<i32, (usize, usize, usize)>,
) -> f64 {
    let Some(elem) = get_element(grid, element_id) else {
        return f64::MAX;
    };
    if installed_nodes.is_empty() {
        return 0.0;
    }
    let cand_nodes = [elem.node_i_id, elem.node_j_id];
    let mut min_d = f64::MAX;
    for &cn in &cand_nodes {
        for &fn_id in installed_nodes {
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

fn element_connectivity(element_id: i32, grid: &SimGrid, installed_nodes: &HashSet<i32>) -> usize {
    let Some(elem) = get_element(grid, element_id) else {
        return 0;
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

fn count_shared_nodes(
    element_ids: &[i32],
    grid: &SimGrid,
    installed_nodes: &HashSet<i32>,
) -> usize {
    let cand_nodes: HashSet<i32> = element_ids
        .iter()
        .flat_map(|eid| {
            get_element(grid, *eid)
                .map(|e| vec![e.node_i_id, e.node_j_id])
                .unwrap_or_default()
        })
        .collect();
    cand_nodes.intersection(installed_nodes).count()
}

fn node_set_for_elements(element_ids: &HashSet<i32>, grid: &SimGrid) -> HashSet<i32> {
    element_ids
        .iter()
        .filter_map(|eid| get_element(grid, *eid))
        .flat_map(|e| [e.node_i_id, e.node_j_id])
        .collect()
}

fn local_xy_positions_by_floor(
    element_ids: &HashSet<i32>,
    node_pos: &HashMap<i32, (usize, usize, usize)>,
    grid: &SimGrid,
) -> HashMap<i32, HashSet<(usize, usize)>> {
    let mut by_floor: HashMap<i32, HashSet<(usize, usize)>> = HashMap::new();

    for eid in element_ids {
        let Some(floor) = grid.element_floor_by_id.get(eid).copied() else {
            continue;
        };
        let Some(elem) = get_element(grid, *eid) else {
            continue;
        };

        let positions = by_floor.entry(floor).or_default();
        for node_id in [elem.node_i_id, elem.node_j_id] {
            if let Some(&(xi, yi, _)) = node_pos.get(&node_id) {
                positions.insert((xi, yi));
            }
        }
    }

    by_floor
}

fn min_xy_distance_to_local_positions(
    candidate_nodes: &[i32],
    node_pos: &HashMap<i32, (usize, usize, usize)>,
    local_positions: &HashSet<(usize, usize)>,
    wf: &SimWorkfront,
) -> f64 {
    if local_positions.is_empty() {
        return candidate_nodes
            .iter()
            .filter_map(|node_id| node_pos.get(node_id))
            .map(|&(xi, yi, _)| {
                ((xi as i32 - wf.grid_x as i32).abs() + (yi as i32 - wf.grid_y as i32).abs())
                    as f64
            })
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or(f64::MAX);
    }

    candidate_nodes
        .iter()
        .filter_map(|node_id| node_pos.get(node_id))
        .map(|&(xi, yi, _)| {
            local_positions
                .iter()
                .map(|&(lx, ly)| {
                    ((xi as i32 - lx as i32).abs() + (yi as i32 - ly as i32).abs()) as f64
                })
                .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .unwrap_or(f64::MAX)
        })
        .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .unwrap_or(f64::MAX)
}

#[cfg(test)]
fn check_single_stability(element_id: i32, grid: &SimGrid, installed_ids: &HashSet<i32>) -> bool {
    let Some(elem) = get_element(grid, element_id) else {
        return false;
    };

    // NOTE: Connectivity (proximity to construction front) is handled by collect_single_candidates,
    // which only returns elements adjacent to installed nodes. Here we only check structural stability.
    // - Ground floor columns (z=0 base): Always stable (on ground)
    // - Upper floor columns: Need support from below (handled by validate_column_support)
    // - Girders: Need two supporting columns (handled by validate_girder_support)

    if elem.member_type == "Column" {
        crate::stability::validate_column_support(elem, &grid.nodes, &grid.elements, installed_ids)
    } else {
        crate::stability::validate_girder_support(elem, &grid.nodes, &grid.elements, installed_ids)
    }
}

fn check_bundle_stability(
    element_ids: &[i32],
    grid: &SimGrid,
    installed_ids: &HashSet<i32>,
) -> bool {
    check_step_bundle_stability(element_ids, &grid.elements, &grid.nodes, installed_ids)
}

fn check_upper_floor_constraint_tracked(
    element_ids: &[i32],
    floor_tracker: &FloorTracker,
    installed_per_floor: &HashMap<i32, usize>,
    upper_floor_column_rate_threshold: f64,
    lower_floor_completion_ratio_threshold: f64,
    lower_floor_forced_completion_threshold: usize,
) -> bool {
    for eid in element_ids {
        let Some(floor) = floor_tracker.column_floor_by_element.get(eid).copied() else {
            continue;
        };

        if floor <= 1 {
            continue;
        }

        let lower_floor = floor - 1;
        let installed_lower = *installed_per_floor.get(&lower_floor).unwrap_or(&0);
        let total_lower = *floor_tracker.total_per_floor.get(&lower_floor).unwrap_or(&0);

        if total_lower == 0 {
            continue;
        }
        if installed_lower >= total_lower {
            continue;
        }

        // If lower floor has only a few columns left, force lower-floor completion first.
        let remaining_lower = total_lower - installed_lower;
        if remaining_lower <= lower_floor_forced_completion_threshold && installed_lower > 0 {
            return false;
        }

        let lower_floor_completion_ratio = installed_lower as f64 / total_lower as f64;
        let skip_ratio_gate = floor >= floor_tracker.max_floor
            || lower_floor_completion_ratio >= lower_floor_completion_ratio_threshold;

        // B + C: relax only ratio gating.
        // - B: top floor (no upper dependent floor)
        // - C: lower floor already sufficiently completed
        if skip_ratio_gate {
            continue;
        }

        if installed_lower > 0 {
            let installed_upper = *installed_per_floor.get(&floor).unwrap_or(&0) + 1;
            let ratio = installed_upper as f64 / installed_lower as f64;
            if ratio > upper_floor_column_rate_threshold {
                return false;
            }
        }
    }

    true
}

#[cfg(test)]
fn check_upper_floor_constraint_legacy(
    element_ids: &[i32],
    grid: &SimGrid,
    installed_ids: &HashSet<i32>,
    threshold: f64,
) -> bool {
    let dz = grid_dz(grid);

    let mut total_per_floor: HashMap<i32, usize> = HashMap::new();
    for elem in grid.elements.iter().filter(|e| e.member_type == "Column") {
        if let Some((_, _, z)) = grid.node_coords(elem.node_i_id) {
            let floor = (z / dz).round() as i32 + 1;
            *total_per_floor.entry(floor).or_insert(0) += 1;
        }
    }

    let mut installed_per_floor: HashMap<i32, usize> = HashMap::new();
    for eid in installed_ids {
        if let Some(elem) = get_element(grid, *eid) {
            if elem.member_type == "Column" {
                if let Some((_, _, z)) = grid.node_coords(elem.node_i_id) {
                    let floor = (z / dz).round() as i32 + 1;
                    *installed_per_floor.entry(floor).or_insert(0) += 1;
                }
            }
        }
    }

    for eid in element_ids {
        let Some(elem) = get_element(grid, *eid) else {
            continue;
        };
        if elem.member_type != "Column" {
            continue;
        }

        let Some((_, _, z)) = grid.node_coords(elem.node_i_id) else {
            continue;
        };
        let floor = (z / dz).round() as i32 + 1;

        if floor <= 1 {
            continue;
        }

        let lower_floor = floor - 1;
        let installed_lower = *installed_per_floor.get(&lower_floor).unwrap_or(&0);
        let total_lower = *total_per_floor.get(&lower_floor).unwrap_or(&0);

        // If lower floor has no columns defined, allow (edge case)
        if total_lower == 0 {
            continue;
        }

        // If lower floor is complete (100%), no constraint needed
        if installed_lower >= total_lower {
            continue;
        }

        // CRITICAL RULE: If lower floor has 5 or fewer uninstalled columns,
        // block upper floor installation until lower floor is complete.
        // BUT: Only apply this rule if lower floor has at least SOME installation progress.
        // If lower floor has 0 installed, we're still in early stages - allow upper floor
        // based on ratio constraint only.
        let remaining_lower = total_lower - installed_lower;
        if remaining_lower <= 5 && installed_lower > 0 {
            return false;
        }

        // Standard ratio constraint for upper/lower floor balance
        // Only apply if lower floor has some progress
        if installed_lower > 0 {
            let installed_upper = *installed_per_floor.get(&floor).unwrap_or(&0) + 1;
            let ratio = installed_upper as f64 / installed_lower as f64;
            if ratio > threshold {
                return false;
            }
        }
    }

    true
}

#[cfg(test)]
fn check_upper_floor_constraint(
    element_ids: &[i32],
    grid: &SimGrid,
    installed_ids: &HashSet<i32>,
    upper_floor_column_rate_threshold: f64,
    lower_floor_completion_ratio_threshold: f64,
    lower_floor_forced_completion_threshold: usize,
) -> bool {
    let dz = grid_dz(grid);
    let tracker = FloorTracker::from_grid(grid, dz);
    let installed_per_floor = tracker.installed_per_floor_from(installed_ids);
    check_upper_floor_constraint_tracked(
        element_ids,
        &tracker,
        &installed_per_floor,
        upper_floor_column_rate_threshold,
        lower_floor_completion_ratio_threshold,
        lower_floor_forced_completion_threshold,
    )
}

fn is_floor_eligible_for_new_work(
    floor: i32,
    installed_columns_per_floor: &HashMap<i32, usize>,
    total_columns_per_floor: &HashMap<i32, usize>,
    constraints: &SimConstraints,
) -> bool {
    if floor <= 1 {
        return true;
    }

    let lower_floor = floor - 1;
    let lower_total_columns = *total_columns_per_floor.get(&lower_floor).unwrap_or(&0);
    let lower_installed_columns = *installed_columns_per_floor.get(&lower_floor).unwrap_or(&0);

    if lower_total_columns == 0 {
        return true;
    }

    let lower_completion_ratio = lower_installed_columns as f64 / lower_total_columns as f64;
    if lower_completion_ratio < constraints.lower_floor_completion_ratio_threshold {
        return false;
    }

    let lower_remaining_columns = lower_total_columns.saturating_sub(lower_installed_columns);

    if lower_remaining_columns >= 1
        && lower_remaining_columns <= constraints.lower_floor_forced_completion_threshold
    {
        return false;
    }

    true
}

fn choose_target_floor(
    candidate_floors: &[i32],
    installed_columns_per_floor: &HashMap<i32, usize>,
    constraints: &SimConstraints,
    avoid_floor: Option<i32>,
) -> i32 {
    let mut filtered_floors: Vec<i32> = candidate_floors
        .iter()
        .copied()
        .filter(|floor| Some(*floor) != avoid_floor)
        .collect();

    if filtered_floors.is_empty() {
        filtered_floors = candidate_floors.to_vec();
    }

    let mut deficit_floors: Vec<(i32, f64)> = filtered_floors
        .iter()
        .filter_map(|floor| {
            if *floor <= 1 {
                return None;
            }

            let lower_floor = *floor - 1;
            let installed_lower = *installed_columns_per_floor.get(&lower_floor).unwrap_or(&0);
            if installed_lower == 0 {
                return None;
            }

            let installed_upper = *installed_columns_per_floor.get(floor).unwrap_or(&0) as f64;
            let target_upper = installed_lower as f64 * constraints.upper_floor_column_rate_threshold;
            let deficit = target_upper - installed_upper;

            (deficit > 0.0).then_some((*floor, deficit))
        })
        .collect();

    if !deficit_floors.is_empty() {
        deficit_floors.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.0.cmp(&a.0))
        });
        return deficit_floors[0].0;
    }

    filtered_floors.into_iter().min().unwrap_or(1)
}

fn collect_single_candidates(
    wf: &SimWorkfront,
    grid: &SimGrid,
    support_ids: &HashSet<i32>,
    local_element_ids: &HashSet<i32>,
    committed_ids: &HashSet<i32>,
    node_pos: &HashMap<i32, (usize, usize, usize)>,
) -> Vec<SingleCandidate> {
    collect_single_candidates_optimized(
        wf,
        grid,
        support_ids,
        local_element_ids,
        committed_ids,
        node_pos,
    )
}

#[cfg(test)]
fn collect_single_candidates_legacy(
    wf: &SimWorkfront,
    grid: &SimGrid,
    support_ids: &HashSet<i32>,
    local_element_ids: &HashSet<i32>,
    committed_ids: &HashSet<i32>,
    node_pos: &HashMap<i32, (usize, usize, usize)>,
) -> Vec<SingleCandidate> {
    let support_nodes = node_set_for_elements(support_ids, grid);
    let local_positions_by_floor = local_xy_positions_by_floor(local_element_ids, node_pos, grid);
    let empty_positions: HashSet<(usize, usize)> = HashSet::new();
    let mut result: Vec<SingleCandidate> = Vec::new();

    for elem in &grid.elements {
        if committed_ids.contains(&elem.id) {
            continue;
        }

        let floor = element_floor(elem.id, grid, grid_dz(grid)).unwrap_or(0);
        let local_positions = local_positions_by_floor
            .get(&floor)
            .unwrap_or(&empty_positions);
        let local_seeded = !local_positions.is_empty();

        let candidate_nodes = [elem.node_i_id, elem.node_j_id];
        let dist = min_xy_distance_to_local_positions(&candidate_nodes, node_pos, &local_positions, wf);

        if !dist.is_finite() {
            continue;
        }

        // If this workfront has no local footprint yet on the target floor:
        // - floor 1 keeps strict anchor start
        // - upper floors allow near-anchor column starts to avoid hard lock
        if !local_seeded && elem.member_type == "Column" {
            let Some(&(xi, yi, _)) = node_pos.get(&elem.node_i_id) else {
                continue;
            };
            if floor <= 1 {
                if xi != wf.grid_x || yi != wf.grid_y {
                    continue;
                }
            } else if dist > 1.0 {
                continue;
            }
        }

        if local_seeded && dist > 1.0 && elem.member_type == "Column" {
            continue;
        }

        let structurally_possible = if elem.member_type == "Column" {
            if let Some(&(_, _, zi)) = node_pos.get(&elem.node_i_id) {
                if zi == 0 {
                    true
                } else {
                    support_nodes.contains(&elem.node_i_id)
                }
            } else {
                false
            }
        } else {
            support_nodes.contains(&elem.node_i_id) || support_nodes.contains(&elem.node_j_id)
        };

        if !structurally_possible {
            continue;
        }

        let connectivity = element_connectivity(elem.id, grid, &support_nodes);
        result.push(SingleCandidate {
            element_id: elem.id,
            connectivity,
            frontier_dist: dist,
        });
    }

    result
}

fn collect_single_candidates_optimized(
    wf: &SimWorkfront,
    grid: &SimGrid,
    support_ids: &HashSet<i32>,
    local_element_ids: &HashSet<i32>,
    committed_ids: &HashSet<i32>,
    node_pos: &HashMap<i32, (usize, usize, usize)>,
) -> Vec<SingleCandidate> {
    let support_nodes = node_set_for_elements(support_ids, grid);
    let local_positions_by_floor = local_xy_positions_by_floor(local_element_ids, node_pos, grid);
    let empty_positions: HashSet<(usize, usize)> = HashSet::new();
    let mut result: Vec<SingleCandidate> = Vec::new();

    let candidate_ids: &[i32] = grid.element_ids_in_order.as_slice();

    for eid in candidate_ids {
        if committed_ids.contains(eid) {
            continue;
        }

        let Some(elem) = get_element(grid, *eid) else {
            continue;
        };

        let floor = grid.element_floor_by_id.get(eid).copied().unwrap_or(0);
        let local_positions = local_positions_by_floor
            .get(&floor)
            .unwrap_or(&empty_positions);
        let local_seeded = !local_positions.is_empty();

        let candidate_nodes = [elem.node_i_id, elem.node_j_id];
        let dist =
            min_xy_distance_to_local_positions(&candidate_nodes, node_pos, local_positions, wf);

        if !dist.is_finite() {
            continue;
        }

        // If this workfront has no local footprint yet on the target floor:
        // - floor 1 keeps strict anchor start
        // - upper floors allow near-anchor column starts to avoid hard lock
        if !local_seeded && elem.member_type == "Column" {
            let Some(&(xi, yi, _)) = node_pos.get(&elem.node_i_id) else {
                continue;
            };
            if floor <= 1 {
                if xi != wf.grid_x || yi != wf.grid_y {
                    continue;
                }
            } else if dist > 1.0 {
                continue;
            }
        }

        if local_seeded && dist > 1.0 && elem.member_type == "Column" {
            continue;
        }

        let structurally_possible = if elem.member_type == "Column" {
            if let Some(&(_, _, zi)) = node_pos.get(&elem.node_i_id) {
                if zi == 0 {
                    true
                } else {
                    support_nodes.contains(&elem.node_i_id)
                }
            } else {
                false
            }
        } else {
            support_nodes.contains(&elem.node_i_id) || support_nodes.contains(&elem.node_j_id)
        };

        if !structurally_possible {
            continue;
        }

        let connectivity = element_connectivity(elem.id, grid, &support_nodes);
        result.push(SingleCandidate {
            element_id: elem.id,
            connectivity,
            frontier_dist: dist,
        });
    }

    result
}

fn generate_bootstrap_candidates(
    wf: &SimWorkfront,
    grid: &SimGrid,
    node_pos: &HashMap<i32, (usize, usize, usize)>,
) -> Vec<Candidate> {
    let anchor_zi = 0usize;
    let upper_zi = 1usize;

    let col_jnode: HashMap<i32, i32> = grid
        .elements
        .iter()
        .filter(|e| e.member_type == "Column")
        .map(|e| (e.id, e.node_j_id))
        .collect();

    let all_floor1_gdrs: Vec<(i32, i32, i32)> = grid
        .elements
        .iter()
        .filter(|e| {
            e.member_type == "Girder"
                && node_pos.get(&e.node_i_id).map(|p| p.2).unwrap_or(999) == upper_zi
        })
        .map(|e| (e.id, e.node_i_id, e.node_j_id))
        .collect();

    let mut candidates: Vec<Candidate> = Vec::new();
    for patch in 1i32..=(grid.nx.max(grid.ny) as i32) {
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

        'outer: for ci in 0..patch_cols.len() {
            for cj in (ci + 1)..patch_cols.len() {
                for ck in (cj + 1)..patch_cols.len() {
                    let c_ids = [patch_cols[ci], patch_cols[cj], patch_cols[ck]];
                    let jnodes: HashSet<i32> = c_ids
                        .iter()
                        .filter_map(|&cid| col_jnode.get(&cid).copied())
                        .collect();
                    if jnodes.len() < 3 {
                        continue;
                    }

                    let valid_gdrs: Vec<i32> = all_floor1_gdrs
                        .iter()
                        .filter(|&&(_, ni, nj)| jnodes.contains(&ni) && jnodes.contains(&nj))
                        .map(|&(gid, _, _)| gid)
                        .collect();

                    for gi in 0..valid_gdrs.len() {
                        for gj in (gi + 1)..valid_gdrs.len() {
                            let gid_a = valid_gdrs[gi];
                            let gid_b = valid_gdrs[gj];
                            if let (Some(&(_, ni_a, nj_a)), Some(&(_, ni_b, nj_b))) = (
                                all_floor1_gdrs.iter().find(|&&(id, _, _)| id == gid_a),
                                all_floor1_gdrs.iter().find(|&&(id, _, _)| id == gid_b),
                            ) {
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
                                    candidates.push(Candidate {
                                        element_ids: vec![
                                            c_ids[0], c_ids[1], c_ids[2], gid_a, gid_b,
                                        ],
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
            break;
        }
    }
    candidates
}

fn push_pattern_if_valid(
    choices: &mut Vec<PatternChoice>,
    seen: &mut HashSet<String>,
    element_ids: Vec<i32>,
    pattern: StepPatternType,
    grid: &SimGrid,
    installed_ids: &HashSet<i32>,
    floor_tracker: &FloorTracker,
    installed_per_floor: &HashMap<i32, usize>,
    upper_floor_column_rate_threshold: f64,
    lower_floor_completion_ratio_threshold: f64,
    lower_floor_forced_completion_threshold: usize,
) {
    if contains_forbidden_pattern(&element_ids, grid) {
        return;
    }
    if element_ids.iter().any(|eid| installed_ids.contains(eid)) {
        return;
    }
    let mut unique = HashSet::new();
    if !element_ids.iter().all(|eid| unique.insert(*eid)) {
        return;
    }
    if !check_bundle_stability(&element_ids, grid, installed_ids) {
        return;
    }
    if !check_upper_floor_constraint_tracked(
        &element_ids,
        floor_tracker,
        installed_per_floor,
        upper_floor_column_rate_threshold,
        lower_floor_completion_ratio_threshold,
        lower_floor_forced_completion_threshold,
    ) {
        return;
    }

    let key = element_ids
        .iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join("-");
    if seen.insert(key) {
        choices.push(PatternChoice {
            element_ids,
            pattern,
        });
    }
}

fn contains_forbidden_pattern(element_ids: &[i32], grid: &SimGrid) -> bool {
    let types: Vec<&str> = element_ids
        .iter()
        .filter_map(|eid| get_element(grid, *eid).map(|e| e.member_type.as_str()))
        .collect();

    // Forbid 3+ consecutive columns (regardless of girders after).
    // All other validity (girder connectivity, parallel girders, etc.) is
    // handled by check_bundle_stability — a single girder or parallel girders
    // are valid as long as they connect to the existing structure and the
    // resulting assembly is stable.
    matches!(
        types.as_slice(),
        ["Column", "Column", "Column"] | ["Column", "Column", "Column", "Girder"]
    )
}

fn element_floor(element_id: i32, grid: &SimGrid, dz: f64) -> Option<i32> {
    let elem = get_element(grid, element_id)?;
    if elem.member_type == "Column" {
        let (_, _, z) = grid.node_coords(elem.node_i_id)?;
        Some((z / dz).round() as i32 + 1)
    } else {
        let (_, _, z) = grid.node_coords(elem.node_i_id)?;
        Some(((z / dz).round() as i32).max(1))
    }
}

fn uninstalled_adjacent_columns(
    column_id: i32,
    grid: &SimGrid,
    installed_ids: &HashSet<i32>,
    node_pos: &HashMap<i32, (usize, usize, usize)>,
) -> Vec<i32> {
    let floor_zi = get_element(grid, column_id)
        .and_then(|e| node_pos.get(&e.node_i_id).map(|pos| pos.2))
        .unwrap_or(0);
    let mut result: Vec<i32> = grid
        .adjacent_columns(column_id, floor_zi)
        .into_iter()
        .filter(|eid| !installed_ids.contains(eid))
        .collect();
    result.sort();
    result.dedup();
    result
}

fn uninstalled_girders_touching_node(
    node_id: i32,
    grid: &SimGrid,
    installed_ids: &HashSet<i32>,
) -> Vec<i32> {
    let mut result: Vec<i32> = grid
        .elements
        .iter()
        .filter(|e| e.member_type == "Girder")
        .filter(|e| !installed_ids.contains(&e.id))
        .filter(|e| e.node_i_id == node_id || e.node_j_id == node_id)
        .map(|e| e.id)
        .collect();
    result.sort();
    result.dedup();
    result
}

fn installed_girders_touching_node(
    node_id: i32,
    grid: &SimGrid,
    installed_ids: &HashSet<i32>,
) -> Vec<i32> {
    let mut result: Vec<i32> = grid
        .elements
        .iter()
        .filter(|e| e.member_type == "Girder")
        .filter(|e| installed_ids.contains(&e.id))
        .filter(|e| e.node_i_id == node_id || e.node_j_id == node_id)
        .map(|e| e.id)
        .collect();
    result.sort();
    result.dedup();
    result
}

fn uninstalled_columns_ending_at_node(
    node_id: i32,
    grid: &SimGrid,
    installed_ids: &HashSet<i32>,
) -> Vec<i32> {
    let mut result: Vec<i32> = grid
        .elements
        .iter()
        .filter(|e| e.member_type == "Column")
        .filter(|e| !installed_ids.contains(&e.id))
        .filter(|e| e.node_j_id == node_id)
        .map(|e| e.id)
        .collect();
    result.sort();
    result.dedup();
    result
}

fn girder_direction(element: &StabilityElement, grid: &SimGrid) -> Option<(i32, i32)> {
    let ni = grid.node_coords(element.node_i_id)?;
    let nj = grid.node_coords(element.node_j_id)?;
    let dx = nj.0 - ni.0;
    let dy = nj.1 - ni.1;

    Some((
        if dx.abs() > 0.001 {
            dx.signum() as i32
        } else {
            0
        },
        if dy.abs() > 0.001 {
            dy.signum() as i32
        } else {
            0
        },
    ))
}

fn is_perpendicular(dir1: (i32, i32), dir2: (i32, i32)) -> bool {
    let dot = dir1.0 * dir2.0 + dir1.1 * dir2.1;
    dot == 0 && (dir1.0 != 0 || dir1.1 != 0) && (dir2.0 != 0 || dir2.1 != 0)
}

fn cross_girder_bonus(element_ids: &[i32], grid: &SimGrid, installed_ids: &HashSet<i32>) -> f64 {
    let mut bonus = 0.0;

    for eid in element_ids {
        let Some(elem) = get_element(grid, *eid) else {
            continue;
        };
        if elem.member_type != "Girder" {
            continue;
        }

        let Some(dir) = girder_direction(elem, grid) else {
            continue;
        };

        for node_id in [elem.node_i_id, elem.node_j_id] {
            let touching = installed_girders_touching_node(node_id, grid, installed_ids);
            let has_perpendicular = touching.iter().any(|gid| {
                get_element(grid, *gid)
                    .and_then(|g| girder_direction(g, grid))
                    .map(|other_dir| is_perpendicular(dir, other_dir))
                    .unwrap_or(false)
            });

            if has_perpendicular {
                bonus += 1.0;
            }
        }
    }

    bonus
}

fn bundle_score(
    element_ids: &[i32],
    grid: &SimGrid,
    installed_nodes: &HashSet<i32>,
    node_pos: &HashMap<i32, (usize, usize, usize)>,
    installed_ids: &HashSet<i32>,
    w1: f64,
    w2: f64,
    w3: f64,
) -> f64 {
    let connectivity_sum: usize = element_ids
        .iter()
        .map(|eid| element_connectivity(*eid, grid, installed_nodes))
        .sum();
    let frontier_dist = element_ids
        .iter()
        .map(|eid| element_frontier_dist(*eid, grid, installed_nodes, node_pos))
        .fold(f64::MAX, f64::min);
    let dist_score = if frontier_dist.is_finite() {
        w2 * (1.0 / (frontier_dist + 1.0))
    } else {
        0.0
    };
    let size_bonus = element_ids.len() as f64 * 0.75;
    let closure_score = (4.0 * w3) * cross_girder_bonus(element_ids, grid, installed_ids);
    w1 * connectivity_sum as f64 + dist_score + size_bonus + closure_score
}

fn try_build_pattern(
    seed_id: i32,
    grid: &SimGrid,
    floor_tracker: &FloorTracker,
    installed_ids: &HashSet<i32>,
    installed_nodes: &HashSet<i32>,
    node_pos: &HashMap<i32, (usize, usize, usize)>,
    upper_floor_column_rate_threshold: f64,
    lower_floor_completion_ratio_threshold: f64,
    lower_floor_forced_completion_threshold: usize,
    w1: f64,
    w2: f64,
    w3: f64,
    rng: &mut u64,
) -> (Vec<i32>, String) {
    let Some(seed) = get_element(grid, seed_id) else {
        return (Vec::new(), StepPatternType::Col.as_str().to_string());
    };

    let mut choices: Vec<PatternChoice> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let installed_per_floor = floor_tracker.installed_per_floor_from(installed_ids);

    if seed.member_type == "Girder" {
        push_pattern_if_valid(
            &mut choices,
            &mut seen,
            vec![seed_id],
            StepPatternType::Girder,
            grid,
            installed_ids,
            floor_tracker,
            &installed_per_floor,
            upper_floor_column_rate_threshold,
            lower_floor_completion_ratio_threshold,
            lower_floor_forced_completion_threshold,
        );

        let mut second_girders =
            uninstalled_girders_touching_node(seed.node_i_id, grid, installed_ids);
        second_girders.extend(uninstalled_girders_touching_node(
            seed.node_j_id,
            grid,
            installed_ids,
        ));
        second_girders.sort();
        second_girders.dedup();

        for g2 in second_girders {
            if g2 == seed_id {
                continue;
            }
            push_pattern_if_valid(
                &mut choices,
                &mut seen,
                vec![seed_id, g2],
                StepPatternType::GirderGirder,
                grid,
                installed_ids,
                floor_tracker,
                &installed_per_floor,
                upper_floor_column_rate_threshold,
                lower_floor_completion_ratio_threshold,
                lower_floor_forced_completion_threshold,
            );
        }
    } else {
        push_pattern_if_valid(
            &mut choices,
            &mut seen,
            vec![seed_id],
            StepPatternType::Col,
            grid,
            installed_ids,
            floor_tracker,
            &installed_per_floor,
            upper_floor_column_rate_threshold,
            lower_floor_completion_ratio_threshold,
            lower_floor_forced_completion_threshold,
        );

        let seed_upper = seed.node_j_id;

        for col2 in uninstalled_adjacent_columns(seed_id, grid, installed_ids, node_pos) {
            push_pattern_if_valid(
                &mut choices,
                &mut seen,
                vec![seed_id, col2],
                StepPatternType::ColCol,
                grid,
                installed_ids,
                floor_tracker,
                &installed_per_floor,
                upper_floor_column_rate_threshold,
                lower_floor_completion_ratio_threshold,
                lower_floor_forced_completion_threshold,
            );

            if let Some(col2_elem) = get_element(grid, col2) {
                if let Some(g1) = grid.girder_between(seed_upper, col2_elem.node_j_id) {
                    if !installed_ids.contains(&g1) {
                        push_pattern_if_valid(
                            &mut choices,
                            &mut seen,
                            vec![seed_id, col2, g1],
                            StepPatternType::ColColGirder,
                            grid,
                            installed_ids,
                            floor_tracker,
                            &installed_per_floor,
                            upper_floor_column_rate_threshold,
                            lower_floor_completion_ratio_threshold,
                            lower_floor_forced_completion_threshold,
                        );

                        let mut second_girders =
                            uninstalled_girders_touching_node(seed_upper, grid, installed_ids);
                        second_girders.extend(uninstalled_girders_touching_node(
                            col2_elem.node_j_id,
                            grid,
                            installed_ids,
                        ));
                        second_girders.sort();
                        second_girders.dedup();

                        for g2 in second_girders {
                            if g2 == g1 {
                                continue;
                            }
                            push_pattern_if_valid(
                                &mut choices,
                                &mut seen,
                                vec![seed_id, col2, g1, g2],
                                StepPatternType::ColColGirderGirder,
                                grid,
                                installed_ids,
                                floor_tracker,
                                &installed_per_floor,
                                upper_floor_column_rate_threshold,
                                lower_floor_completion_ratio_threshold,
                                lower_floor_forced_completion_threshold,
                            );
                        }

                        let mut col3_candidates =
                            uninstalled_adjacent_columns(seed_id, grid, installed_ids, node_pos);
                        col3_candidates.extend(uninstalled_adjacent_columns(
                            col2,
                            grid,
                            installed_ids,
                            node_pos,
                        ));
                        col3_candidates.sort();
                        col3_candidates.dedup();

                        for col3 in col3_candidates {
                            if col3 == seed_id || col3 == col2 {
                                continue;
                            }
                            if let Some(col3_elem) = get_element(grid, col3) {
                                for maybe_g2 in [
                                    grid.girder_between(seed_upper, col3_elem.node_j_id),
                                    grid.girder_between(col2_elem.node_j_id, col3_elem.node_j_id),
                                ] {
                                    if let Some(g2) = maybe_g2 {
                                        if g2 == g1 || installed_ids.contains(&g2) {
                                            continue;
                                        }
                                        push_pattern_if_valid(
                                            &mut choices,
                                            &mut seen,
                                            vec![seed_id, col2, g1, col3, g2],
                                            StepPatternType::ColColGirderColGirder,
                                            grid,
                                            installed_ids,
                                            floor_tracker,
                                            &installed_per_floor,
                                            upper_floor_column_rate_threshold,
                                            lower_floor_completion_ratio_threshold,
                                            lower_floor_forced_completion_threshold,
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        for g1 in uninstalled_girders_touching_node(seed_upper, grid, installed_ids) {
            push_pattern_if_valid(
                &mut choices,
                &mut seen,
                vec![seed_id, g1],
                StepPatternType::ColGirder,
                grid,
                installed_ids,
                floor_tracker,
                &installed_per_floor,
                upper_floor_column_rate_threshold,
                lower_floor_completion_ratio_threshold,
                lower_floor_forced_completion_threshold,
            );

            if let Some(g1_elem) = get_element(grid, g1) {
                if let Some(g1_other_node) = other_node(g1_elem, seed_upper) {
                    for col2 in
                        uninstalled_columns_ending_at_node(g1_other_node, grid, installed_ids)
                    {
                        push_pattern_if_valid(
                            &mut choices,
                            &mut seen,
                            vec![seed_id, g1, col2],
                            StepPatternType::ColGirderCol,
                            grid,
                            installed_ids,
                            floor_tracker,
                            &installed_per_floor,
                            upper_floor_column_rate_threshold,
                            lower_floor_completion_ratio_threshold,
                            lower_floor_forced_completion_threshold,
                        );
                    }

                    let mut second_girders =
                        uninstalled_girders_touching_node(seed_upper, grid, installed_ids);
                    second_girders.extend(uninstalled_girders_touching_node(
                        g1_other_node,
                        grid,
                        installed_ids,
                    ));
                    second_girders.sort();
                    second_girders.dedup();
                    for g2 in second_girders {
                        if g2 == g1 {
                            continue;
                        }
                        push_pattern_if_valid(
                            &mut choices,
                            &mut seen,
                            vec![seed_id, g1, g2],
                            StepPatternType::ColGirderGirder,
                            grid,
                            installed_ids,
                            floor_tracker,
                            &installed_per_floor,
                                upper_floor_column_rate_threshold,
                                lower_floor_completion_ratio_threshold,
                            lower_floor_forced_completion_threshold,
                        );
                    }
                }
            }
        }
    }

    if choices.is_empty() {
        return if seed.member_type == "Girder" {
            (vec![seed_id], StepPatternType::Girder.as_str().to_string())
        } else {
            (vec![seed_id], StepPatternType::Col.as_str().to_string())
        };
    }

    let max_len = choices
        .iter()
        .map(|c| c.element_ids.len())
        .max()
        .unwrap_or(1);
    let longest: Vec<&PatternChoice> = choices
        .iter()
        .filter(|choice| choice.element_ids.len() == max_len)
        .collect();
    let scores: Vec<f64> = longest
        .iter()
        .map(|choice| {
            bundle_score(
                &choice.element_ids,
                grid,
                installed_nodes,
                node_pos,
                installed_ids,
                w1,
                w2,
                w3,
            )
        })
        .collect();
    let chosen = longest[weighted_random_choice(&scores, rng)];
    (
        chosen.element_ids.clone(),
        chosen.pattern.as_str().to_string(),
    )
}

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

fn run_scenario_internal(
    scenario_id: usize,
    grid: &SimGrid,
    workfronts: &[SimWorkfront],
    seed: u64,
    weights: (f64, f64, f64),
    constraints: SimConstraints,
    cancel_flag: Option<&AtomicBool>,
) -> SimScenario {
    let (w1, w2, w3) = weights;
    let mut rng = seed;

    let mut stable_ids: HashSet<i32> = HashSet::new();
    let mut steps: Vec<SimStep> = Vec::new();
    let mut workfront_states: HashMap<i32, WorkfrontState> = workfronts
        .iter()
        .map(|wf| (wf.id, WorkfrontState::default()))
        .collect();

    let total_elements = grid.elements.len();
    let node_pos = build_node_pos(grid);
    let dz = grid_dz(grid);
    let floor_tracker = FloorTracker::from_grid(grid, dz);

    let mut consecutive_empty_cycles = 0u32;
    let mut next_sequence_start: usize = 1; // 1-based sequence numbering for from_local_steps
    let mut total_sequence_rounds: usize = 0; // for stagnation/max-iteration check

    let termination_reason = 'outer: loop {
        if cancel_flag
            .map(|flag| flag.load(Ordering::Relaxed))
            .unwrap_or(false)
        {
            break TerminationReason::Cancelled;
        }

        // ── Termination checks ──────────────────────────────────────
        let committed_ids: HashSet<i32> = workfront_states.values().fold(stable_ids.clone(), |mut acc, state| {
            acc.extend(state.buffer_sequences.iter().map(|seq| seq.element_id));
            acc
        });

        if stable_ids.len() >= total_elements {
            break TerminationReason::Completed;
        }
        if committed_ids.len() >= total_elements && stable_ids.len() < total_elements {
            break TerminationReason::NoCandidates;
        }
        if workfronts.is_empty() {
            break TerminationReason::NoCandidates;
        }

        // ── Global Step Cycle: collect local steps from all workfronts ──
        let mut cycle_local_steps: Vec<LocalStep> = Vec::new();
        let mut cycle_completed_wf: HashSet<i32> = HashSet::new();
        let mut cycle_no_progress_count = 0u32;

        loop {
            if cancel_flag
                .map(|flag| flag.load(Ordering::Relaxed))
                .unwrap_or(false)
            {
                break 'outer TerminationReason::Cancelled;
            }

            // Eligible workfronts: those that haven't completed a local step in this cycle
            let eligible_wfs: Vec<&SimWorkfront> = workfronts
                .iter()
                .filter(|wf| !cycle_completed_wf.contains(&wf.id))
                .collect();

            if eligible_wfs.is_empty() {
                break; // All workfronts completed or excluded
            }

            // Recompute committed IDs including in-cycle installations
            let committed_ids: HashSet<i32> = workfront_states.values().fold(stable_ids.clone(), |mut acc, state| {
                acc.extend(state.buffer_sequences.iter().map(|seq| seq.element_id));
                acc
            });
            let committed_floor_counts = floor_tracker.installed_per_floor_from(&committed_ids);

            total_sequence_rounds += 1;

            let mut selected_this_sequence: HashSet<i32> = HashSet::new();
            let mut sequence_installations: Vec<(i32, i32)> = Vec::new(); // (wf_id, element_id)
            // Each eligible workfront selects one element
            for wf in &eligible_wfs {
                let Some(current_state) = workfront_states.get(&wf.id) else {
                    continue;
                };

                let own_buffer_ids: HashSet<i32> = current_state
                    .buffer_sequences
                    .iter()
                    .map(|seq| seq.element_id)
                    .collect();
                let planned_reserved_ids: HashSet<i32> = workfront_states
                    .iter()
                    .filter(|(other_wf_id, _)| **other_wf_id != wf.id)
                    .flat_map(|(_, state)| state.planned_pattern.iter().copied())
                    .collect();
                let plan_has_conflict = current_state.planned_pattern.iter().any(|eid| {
                    !own_buffer_ids.contains(eid)
                        && (committed_ids.contains(eid)
                            || selected_this_sequence.contains(eid)
                            || planned_reserved_ids.contains(eid))
                });
                let plan_exhausted = !current_state.planned_pattern.is_empty()
                    && current_state
                        .planned_pattern
                        .iter()
                        .all(|eid| own_buffer_ids.contains(eid));

                if current_state.planned_pattern.is_empty() || plan_has_conflict || plan_exhausted {
                    let mut rollback_commitment = false;
                    let mut rollback_buffer_ids: Vec<i32> = Vec::new();

                    let new_plan: Vec<i32> = if stable_ids.is_empty() && cycle_local_steps.is_empty() {
                        let bootstrap_candidates: Vec<Candidate> = generate_bootstrap_candidates(wf, grid, &node_pos)
                            .into_iter()
                            .filter(|candidate| {
                                candidate
                                    .element_ids
                                    .iter()
                                    .all(|eid| {
                                        !committed_ids.contains(eid)
                                            && !selected_this_sequence.contains(eid)
                                            && !planned_reserved_ids.contains(eid)
                                    })
                            })
                            .collect();

                        if bootstrap_candidates.is_empty() {
                            Vec::new()
                        } else {
                            let anchor_col = grid.column_starting_at(wf.grid_x, wf.grid_y, 0);
                            let preferred: Vec<&Candidate> = if let Some(anchor_id) = anchor_col {
                                let hits: Vec<&Candidate> = bootstrap_candidates
                                    .iter()
                                    .filter(|candidate| candidate.element_ids.contains(&anchor_id))
                                    .collect();
                                if hits.is_empty() {
                                    bootstrap_candidates.iter().collect()
                                } else {
                                    hits
                                }
                            } else {
                                bootstrap_candidates.iter().collect()
                            };

                            let scores: Vec<f64> = preferred
                                .iter()
                                .map(|candidate| candidate.score(w1, w2, w3))
                                .collect();
                            let chosen_idx = weighted_random_choice(&scores, &mut rng);
                            reorder_bootstrap_pattern(
                                &preferred[chosen_idx].element_ids,
                                grid,
                                &node_pos,
                                wf,
                            )
                        }
                    } else {
                        let mut wf_committed_ids = committed_ids.clone();
                        wf_committed_ids.extend(selected_this_sequence.iter().copied());

                        // Use stable_ids + already-completed cycle local steps as support
                        let mut support_ids = stable_ids.clone();
                        for ls in &cycle_local_steps {
                            support_ids.extend(ls.element_ids.iter().copied());
                        }

                        let local_ids = current_state.all_local_ids();
                        let support_nodes = node_set_for_elements(&support_ids, grid);
                        let wf_candidates = collect_single_candidates(
                            wf,
                            grid,
                            &support_ids,
                            &local_ids,
                            &wf_committed_ids,
                            &node_pos,
                        );

                        let committed_floor = current_state.committed_floor;

                        let valid_seeds: Vec<&SingleCandidate> = wf_candidates
                            .iter()
                            .filter(|candidate| !selected_this_sequence.contains(&candidate.element_id))
                            .filter(|candidate| {
                                check_upper_floor_constraint_tracked(
                                    &[candidate.element_id],
                                    &floor_tracker,
                                    &committed_floor_counts,
                                    constraints.upper_floor_column_rate_threshold,
                                    constraints.lower_floor_completion_ratio_threshold,
                                    constraints.lower_floor_forced_completion_threshold,
                                )
                            })
                            .filter(|candidate| {
                                let candidate_floor = grid
                                    .element_floor_by_id
                                    .get(&candidate.element_id)
                                    .copied()
                                    .unwrap_or(1);

                                if let Some(locked_floor) = committed_floor {
                                    return candidate_floor == locked_floor;
                                }

                                is_floor_eligible_for_new_work(
                                    candidate_floor,
                                    &committed_floor_counts,
                                    &floor_tracker.total_per_floor,
                                    &constraints,
                                )
                            })
                            .collect();

                        if valid_seeds.is_empty() {
                            if committed_floor.is_some() {
                                rollback_commitment = true;
                                rollback_buffer_ids = current_state.buffer_element_ids();
                            }
                            Vec::new()
                        } else {
                            let target_floor = if let Some(locked_floor) = committed_floor {
                                locked_floor
                            } else {
                                let mut floors: Vec<i32> = valid_seeds
                                    .iter()
                                    .filter_map(|candidate| {
                                        grid.element_floor_by_id
                                            .get(&candidate.element_id)
                                            .copied()
                                            .or_else(|| element_floor(candidate.element_id, grid, dz))
                                    })
                                    .collect();
                                floors.sort_unstable();
                                floors.dedup();

                                let avoid_floor = current_state.last_failed_floor;
                                choose_target_floor(
                                    &floors,
                                    &committed_floor_counts,
                                    &constraints,
                                    avoid_floor,
                                )
                            };

                            let floor_seeds: Vec<&SingleCandidate> = valid_seeds
                                .into_iter()
                                .filter(|candidate| {
                                    grid.element_floor_by_id
                                        .get(&candidate.element_id)
                                        .copied()
                                        .or_else(|| element_floor(candidate.element_id, grid, dz))
                                        .unwrap_or(1)
                                        == target_floor
                                })
                                .collect();

                            if floor_seeds.is_empty() {
                                if committed_floor.is_some() {
                                    rollback_commitment = true;
                                    rollback_buffer_ids = current_state.buffer_element_ids();
                                }
                                Vec::new()
                            } else {
                            let mut complete_plans: Vec<(Vec<i32>, f64)> = Vec::new();

                            for seed_candidate in &floor_seeds {
                                let seed_id = seed_candidate.element_id;
                                let seed_is_column = is_column(grid, seed_id);

                                if !seed_is_column && check_bundle_stability(&[seed_id], grid, &support_ids) {
                                    let score = bundle_score(
                                        &[seed_id],
                                        grid,
                                        &support_nodes,
                                        &node_pos,
                                        &support_ids,
                                        w1,
                                        w2,
                                        w3,
                                    );
                                    complete_plans.push((vec![seed_id], score));
                                }

                                if seed_is_column {
                                    let Some(seed_elem) = get_element(grid, seed_id) else {
                                        continue;
                                    };
                                    let seed_upper = seed_elem.node_j_id;
                                    let mut touching_girders = uninstalled_girders_touching_node(
                                        seed_upper,
                                        grid,
                                        &wf_committed_ids,
                                    );
                                    touching_girders.retain(|gid| !planned_reserved_ids.contains(gid));

                                    for &g1 in &touching_girders {
                                        let plan = vec![seed_id, g1];
                                        if check_bundle_stability(&plan, grid, &support_ids) {
                                            let score = bundle_score(
                                                &plan,
                                                grid,
                                                &support_nodes,
                                                &node_pos,
                                                &support_ids,
                                                w1,
                                                w2,
                                                w3,
                                            );
                                            complete_plans.push((plan, score));
                                        }
                                    }

                                    for i in 0..touching_girders.len() {
                                        for j in (i + 1)..touching_girders.len() {
                                            let plan = vec![seed_id, touching_girders[i], touching_girders[j]];
                                            if check_bundle_stability(&plan, grid, &support_ids) {
                                                let score = bundle_score(
                                                    &plan,
                                                    grid,
                                                    &support_nodes,
                                                    &node_pos,
                                                    &support_ids,
                                                    w1,
                                                    w2,
                                                    w3,
                                                );
                                                complete_plans.push((plan, score));
                                            }
                                        }
                                    }
                                }
                            }

                            if !complete_plans.is_empty() {
                                complete_plans.sort_by(|a, b| {
                                    b.1.partial_cmp(&a.1)
                                        .unwrap_or(std::cmp::Ordering::Equal)
                                });
                                complete_plans[0].0.clone()
                            } else {
                                let stable_nodes = node_set_for_elements(&support_ids, grid);
                                let mut seed_order: Vec<&SingleCandidate> = floor_seeds;
                                seed_order.sort_by(|a, b| {
                                    b.score(w1, w2)
                                        .partial_cmp(&a.score(w1, w2))
                                        .unwrap_or(std::cmp::Ordering::Equal)
                                });

                                let mut chosen_plan = Vec::new();
                                for seed_candidate in &seed_order {
                                    let mut plan_rng = rng ^ seed_candidate.element_id as u64;
                                    let (candidate_plan, _) = try_build_pattern(
                                        seed_candidate.element_id,
                                        grid,
                                        &floor_tracker,
                                        &support_ids,
                                        &stable_nodes,
                                        &node_pos,
                                        constraints.upper_floor_column_rate_threshold,
                                        constraints.lower_floor_completion_ratio_threshold,
                                        constraints.lower_floor_forced_completion_threshold,
                                        w1,
                                        w2,
                                        w3,
                                        &mut plan_rng,
                                    );

                                    let is_complete = matches!(
                                        classify_buffer(&candidate_plan, grid, !support_ids.is_empty()),
                                        StepBufferDecision::Complete(_)
                                    );
                                    let is_available = candidate_plan.iter().all(|eid| {
                                        !wf_committed_ids.contains(eid) && !planned_reserved_ids.contains(eid)
                                    });

                                    if is_complete && is_available {
                                        chosen_plan = candidate_plan;
                                        break;
                                    }
                                }

                                // If no complete plan exists yet, start with one seed and keep
                                // extending within the committed floor across subsequent rounds.
                                if chosen_plan.is_empty() {
                                    if let Some(seed_candidate) = seed_order.first() {
                                        chosen_plan.push(seed_candidate.element_id);
                                    }
                                }

                                chosen_plan
                            }
                            }
                        }
                    };

                    if let Some(state) = workfront_states.get_mut(&wf.id) {
                        if rollback_commitment {
                            let failed_floor = state.committed_floor;
                            for eid in &rollback_buffer_ids {
                                state.owned_ids.remove(eid);
                            }
                            state.buffer_sequences.clear();
                            state.planned_pattern.clear();
                            state.committed_floor = None;
                            state.last_failed_floor = failed_floor;
                        }
                        if state.planned_pattern.is_empty() || plan_has_conflict || plan_exhausted {
                            state.planned_pattern = new_plan;
                        }
                    }
                }

                let Some(state) = workfront_states.get(&wf.id) else {
                    continue;
                };

                let next_element = state
                    .planned_pattern
                    .iter()
                    .copied()
                    .find(|eid| {
                        !state.buffer_sequences.iter().any(|seq| seq.element_id == *eid)
                            && !selected_this_sequence.contains(eid)
                    });

                let Some(chosen_eid) = next_element else {
                    continue;
                };

                selected_this_sequence.insert(chosen_eid);
                sequence_installations.push((wf.id, chosen_eid));
            }

            // If no workfront could select anything, this sequence round is empty
            if sequence_installations.is_empty() {
                cycle_no_progress_count += 1;
                if cycle_no_progress_count >= 10 {
                    break; // Stagnation within cycle
                }
                continue;
            }
            cycle_no_progress_count = 0;

            // Add selected elements to workfront buffers
            for &(wf_id, element_id) in &sequence_installations {
                if let Some(state) = workfront_states.get_mut(&wf_id) {
                    let was_empty = state.buffer_sequences.is_empty();
                    state.owned_ids.insert(element_id);
                    state.buffer_sequences.push(SimSequence {
                        element_id,
                        sequence_number: 0, // placeholder — will be reassigned by from_local_steps
                    });
                    if was_empty {
                        state.committed_floor = grid
                            .element_floor_by_id
                            .get(&element_id)
                            .copied()
                            .or_else(|| element_floor(element_id, grid, dz));
                        state.last_failed_floor = None;
                    }
                }
            }

            // Check each eligible workfront for pattern completion
            // Use stable_ids + already-completed cycle local steps as context
            let mut cycle_stable_context: HashSet<i32> = stable_ids.clone();
            for ls in &cycle_local_steps {
                cycle_stable_context.extend(ls.element_ids.iter().copied());
            }

            for wf in &eligible_wfs {
                if cycle_completed_wf.contains(&wf.id) {
                    continue;
                }
                let Some(state) = workfront_states.get_mut(&wf.id) else {
                    continue;
                };

                let buffer_element_ids = state.buffer_element_ids();
                let decision = classify_buffer(&buffer_element_ids, grid, !cycle_stable_context.is_empty());

                if let StepBufferDecision::Complete(pattern) = decision {
                    if check_bundle_stability(&buffer_element_ids, grid, &cycle_stable_context) {
                        let step_floor = buffer_element_ids
                            .iter()
                            .filter_map(|eid| element_floor(*eid, grid, dz))
                            .min()
                            .unwrap_or(1);

                        cycle_local_steps.push(LocalStep {
                            workfront_id: wf.id,
                            element_ids: buffer_element_ids.clone(),
                            floor: step_floor,
                            pattern: pattern.as_str().to_string(),
                        });

                        // Mark this workfront as completed for this cycle
                        cycle_completed_wf.insert(wf.id);

                        // Clear its buffer and plan
                        state.buffer_sequences.clear();
                        state.planned_pattern.clear();
                        state.committed_floor = None;
                        state.last_failed_floor = None;
                    }
                }
            }

            // If all workfronts either completed or are excluded, end cycle
            if cycle_completed_wf.len() >= eligible_wfs.len() {
                break;
            }

            // Safety: max inner iterations per cycle
            if total_sequence_rounds as usize >= total_elements * 10 + 1000 {
                break 'outer TerminationReason::MaxIterations;
            }
        }

        // ── Emit Global Step from collected local steps ─────────────
        if cycle_local_steps.is_empty() {
            consecutive_empty_cycles += 1;
            if consecutive_empty_cycles >= 5 {
                break TerminationReason::NoCandidates;
            }
            // Check absolute stagnation
            if total_sequence_rounds as usize >= total_elements * 10 + 1000 {
                break TerminationReason::MaxIterations;
            }
            continue;
        }
        consecutive_empty_cycles = 0;

        let step = SimStep::from_local_steps(cycle_local_steps, next_sequence_start);
        let round_count = step.sequence_round_count();
        // Advance sequence counter by the number of rounds used in this step.
        next_sequence_start += round_count;

        stable_ids.extend(step.element_ids.iter().copied());
        steps.push(step);
    };

    let total_steps = steps.len();
    let total_members: usize = steps.iter().map(|s| s.element_ids.len()).sum();
    let avg_members_per_step = if total_steps > 0 {
        total_members as f64 / total_steps as f64
    } else {
        0.0
    };

    let avg_connectivity = if steps.is_empty() {
        0.0
    } else {
        let mut cumulative: HashSet<i32> = HashSet::new();
        let total_conn: f64 = steps
            .iter()
            .map(|step| {
                let conn = count_shared_nodes(&step.element_ids, grid, &cumulative) as f64;
                for eid in &step.element_ids {
                    if let Some(e) = get_element(grid, *eid) {
                        cumulative.insert(e.node_i_id);
                        cumulative.insert(e.node_j_id);
                    }
                }
                conn
            })
            .sum();
        total_conn / total_steps as f64
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

pub fn run_scenario(
    scenario_id: usize,
    grid: &SimGrid,
    workfronts: &[SimWorkfront],
    seed: u64,
    weights: (f64, f64, f64),
    threshold: f64,
) -> SimScenario {
    let constraints = SimConstraints {
        upper_floor_column_rate_threshold: threshold,
        lower_floor_completion_ratio_threshold: 0.8,
        lower_floor_forced_completion_threshold: 5,
    };
    run_scenario_internal(scenario_id, grid, workfronts, seed, weights, constraints, None)
}

pub fn run_all_scenarios(
    count: usize,
    grid: &SimGrid,
    workfronts: &[SimWorkfront],
    weights: (f64, f64, f64),
    threshold: f64,
) -> Vec<SimScenario> {
    run_all_scenarios_with_progress(count, grid, workfronts, weights, threshold, None)
}

pub fn run_all_scenarios_with_progress(
    count: usize,
    grid: &SimGrid,
    workfronts: &[SimWorkfront],
    weights: (f64, f64, f64),
    threshold: f64,
    progress_counter: Option<Arc<AtomicUsize>>,
) -> Vec<SimScenario> {
    let constraints = SimConstraints {
        upper_floor_column_rate_threshold: threshold,
        lower_floor_completion_ratio_threshold: 0.8,
        lower_floor_forced_completion_threshold: 5,
    };
    run_all_scenarios_with_progress_and_cancel(
        count,
        grid,
        workfronts,
        weights,
        constraints,
        progress_counter,
        None,
    )
}

pub fn run_all_scenarios_with_progress_and_cancel(
    count: usize,
    grid: &SimGrid,
    workfronts: &[SimWorkfront],
    weights: (f64, f64, f64),
    constraints: SimConstraints,
    progress_counter: Option<Arc<AtomicUsize>>,
    cancel_flag: Option<Arc<AtomicBool>>,
) -> Vec<SimScenario> {
    let mut scenarios: Vec<SimScenario> = (1..=count)
        .into_par_iter()
        .map(|i| {
            if cancel_flag
                .as_ref()
                .map(|flag| flag.load(Ordering::Relaxed))
                .unwrap_or(false)
            {
                return SimScenario {
                    id: i,
                    seed: i as u64 * 2654435761,
                    steps: Vec::new(),
                    metrics: ScenarioMetrics {
                        avg_members_per_step: 0.0,
                        avg_connectivity: 0.0,
                        total_steps: 0,
                        total_members_installed: 0,
                        termination_reason: TerminationReason::Cancelled,
                    },
                };
            }

            let seed = i as u64 * 2654435761;
            let scenario = run_scenario_internal(
                i,
                grid,
                workfronts,
                seed,
                weights,
                constraints,
                cancel_flag.as_deref(),
            );
            if let Some(counter) = &progress_counter {
                counter.fetch_add(1, Ordering::Relaxed);
            }
            scenario
        })
        .collect();
    scenarios.sort_by_key(|s| s.id);
    scenarios
}

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
    fn test_forbidden_pattern_detection() {
        let grid = make_grid_2x2x2();
        assert!(contains_forbidden_pattern(&[1, 2, 3], &grid));
        assert!(!contains_forbidden_pattern(&[1, 2], &grid));
    }

    #[test]
    fn test_sim_step_sequences_are_global_and_aligned() {
        let step = SimStep::from_elements(1, vec![7, 9, 11], 2, "ColGirderGirder", 4);
        assert_eq!(step.pattern, "ColGirderGirder");
        assert_eq!(step.element_ids, vec![7, 9, 11]);
        assert_eq!(step.sequences.len(), 3);
        assert_eq!(step.sequences[0].element_id, 7);
        assert_eq!(step.sequences[0].sequence_number, 4);
        assert_eq!(step.sequences[2].sequence_number, 6);
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
            assert_eq!(
                step.element_ids,
                step.sequences
                    .iter()
                    .map(|s| s.element_id)
                    .collect::<Vec<_>>()
            );
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
    fn test_ab_run_all_scenarios_legacy_vs_with_progress() {
        let grid = make_grid_2x2x2();
        let wfs = make_workfronts_2x2();

        let legacy = run_all_scenarios(4, &grid, &wfs, (0.5, 0.3, 0.2), 0.3);
        let progress = Arc::new(AtomicUsize::new(0));
        let with_progress = run_all_scenarios_with_progress(
            4,
            &grid,
            &wfs,
            (0.5, 0.3, 0.2),
            0.3,
            Some(progress.clone()),
        );

        assert_eq!(progress.load(Ordering::Relaxed), 4);
        assert_eq!(legacy.len(), with_progress.len());

        let legacy_sig: Vec<(usize, usize, usize, String)> = legacy
            .iter()
            .map(|s| {
                (
                    s.id,
                    s.metrics.total_steps,
                    s.metrics.total_members_installed,
                    format!("{:?}", s.metrics.termination_reason),
                )
            })
            .collect();
        let progress_sig: Vec<(usize, usize, usize, String)> = with_progress
            .iter()
            .map(|s| {
                (
                    s.id,
                    s.metrics.total_steps,
                    s.metrics.total_members_installed,
                    format!("{:?}", s.metrics.termination_reason),
                )
            })
            .collect();
        assert_eq!(legacy_sig, progress_sig);
    }

    #[test]
    fn test_grid_dz_uses_cached_config_value() {
        let grid = SimGrid::new(2, 2, 3, 6000.0, 6000.0, 4250.0);
        assert!((grid_dz(&grid) - 4250.0).abs() < 1e-9);
    }

    #[test]
    fn test_scenario_ids_one_indexed() {
        let grid = make_grid_2x2x2();
        let wfs = make_workfronts_2x2();
        let scenarios = run_all_scenarios(3, &grid, &wfs, (0.5, 0.3, 0.2), 0.3);
        let ids: Vec<usize> = scenarios.iter().map(|s| s.id).collect();
        assert_eq!(ids, vec![1, 2, 3]);
    }

    #[test]
    fn test_sequence_numbers_increase_across_steps() {
        let grid = make_grid_2x2x2();
        let wfs = make_workfronts_2x2();
        let scenario = run_scenario(1, &grid, &wfs, 42, (0.5, 0.3, 0.2), 0.3);
        let numbers: Vec<usize> = scenario
            .steps
            .iter()
            .flat_map(|step| step.sequences.iter().map(|s| s.sequence_number))
            .collect();
        let expected: Vec<usize> = (1..=numbers.len()).collect();
        assert_eq!(numbers, expected);
    }

    #[test]
    fn test_multiple_workfronts_start_near_their_positions() {
        let grid = SimGrid::new(4, 4, 2, 6000.0, 6000.0, 4000.0);
        let node_pos = build_node_pos(&grid);
        let wfs = vec![
            SimWorkfront {
                id: 1,
                grid_x: 0,
                grid_y: 0,
            },
            SimWorkfront {
                id: 2,
                grid_x: 3,
                grid_y: 3,
            },
        ];

        let scenario = run_scenario(1, &grid, &wfs, 42, (0.5, 0.3, 0.2), 0.5);
        let mut by_sequence: Vec<(usize, i32)> = scenario
            .steps
            .iter()
            .flat_map(|step| step.sequences.iter().map(|seq| (seq.sequence_number, seq.element_id)))
            .collect();
        by_sequence.sort_by_key(|(seq, _)| *seq);

        let first_two: Vec<i32> = by_sequence.iter().take(2).map(|(_, eid)| *eid).collect();
        assert_eq!(first_two.len(), 2, "two workfronts should install two early elements");

        let positions: Vec<(usize, usize)> = first_two
            .iter()
            .filter_map(|eid| get_element(&grid, *eid))
            .filter_map(|elem| node_pos.get(&elem.node_i_id).map(|&(xi, yi, _)| (xi, yi)))
            .collect();

        let has_near_first = positions.iter().any(|&(xi, yi)| {
            (xi as i32 - 0).abs() + (yi as i32 - 0).abs() <= 1
        });
        let has_near_second = positions.iter().any(|&(xi, yi)| {
            (xi as i32 - 3).abs() + (yi as i32 - 3).abs() <= 1
        });

        assert!(has_near_first, "one early element should start near workfront (0,0)");
        assert!(has_near_second, "one early element should start near workfront (3,3)");
    }

    #[test]
    fn test_multiple_workfronts_share_same_sequence_round() {
        let grid = SimGrid::new(4, 4, 2, 6000.0, 6000.0, 4000.0);
        let wfs = vec![
            SimWorkfront {
                id: 1,
                grid_x: 0,
                grid_y: 0,
            },
            SimWorkfront {
                id: 2,
                grid_x: 3,
                grid_y: 3,
            },
            SimWorkfront {
                id: 3,
                grid_x: 0,
                grid_y: 3,
            },
        ];

        let scenario = run_scenario(1, &grid, &wfs, 42, (0.5, 0.3, 0.2), 0.5);
        let mut per_sequence: HashMap<usize, usize> = HashMap::new();
        for sequence_number in scenario
            .steps
            .iter()
            .flat_map(|step| step.sequences.iter().map(|seq| seq.sequence_number))
        {
            *per_sequence.entry(sequence_number).or_insert(0) += 1;
        }

        assert!(
            per_sequence.values().any(|count| *count >= 2),
            "multiple workfronts should install at least two members in the same sequence round"
        );
    }

    #[test]
    fn test_sequence_numbers_are_round_based_and_non_decreasing() {
        let grid = SimGrid::new(4, 4, 2, 6000.0, 6000.0, 4000.0);
        let wfs = vec![
            SimWorkfront {
                id: 1,
                grid_x: 0,
                grid_y: 0,
            },
            SimWorkfront {
                id: 2,
                grid_x: 3,
                grid_y: 3,
            },
        ];

        let scenario = run_scenario(1, &grid, &wfs, 99, (0.5, 0.3, 0.2), 0.5);
        let numbers: Vec<usize> = scenario
            .steps
            .iter()
            .flat_map(|step| step.sequences.iter().map(|seq| seq.sequence_number))
            .collect();
        let mut unique_numbers = numbers.clone();
        unique_numbers.sort_unstable();
        unique_numbers.dedup();

        assert!(!numbers.is_empty(), "scenario should contain sequence entries");
        assert_eq!(unique_numbers[0], 1, "sequence numbering should start at 1");
        assert!(
            unique_numbers
                .windows(2)
                .all(|pair| pair[1] == pair[0] + 1),
            "sequence round numbers should remain contiguous without gaps"
        );
    }

    #[test]
    fn test_steps_are_emitted_per_workfront_buffer() {
        let grid = SimGrid::new(4, 4, 2, 6000.0, 6000.0, 4000.0);
        let wfs = vec![
            SimWorkfront {
                id: 1,
                grid_x: 0,
                grid_y: 0,
            },
            SimWorkfront {
                id: 2,
                grid_x: 3,
                grid_y: 3,
            },
        ];

        let scenario = run_scenario(1, &grid, &wfs, 21, (0.5, 0.3, 0.2), 0.5);
        assert!(
            scenario.steps.iter().all(|step| step.workfront_id >= 1),
            "steps should belong to individual workfront buffers, not a mixed shared step"
        );
    }

    #[test]
    fn test_disconnected_independent_bootstrap_still_passes_with_far_stable_structure() {
        let grid = SimGrid::new(4, 4, 2, 6000.0, 6000.0, 4000.0);

        let col_a = grid.column_starting_at(0, 0, 0).unwrap();
        let col_b = grid.column_starting_at(1, 0, 0).unwrap();
        let col_c = grid.column_starting_at(0, 1, 0).unwrap();
        let node_a = get_element(&grid, col_a).unwrap().node_j_id;
        let node_b = get_element(&grid, col_b).unwrap().node_j_id;
        let node_c = get_element(&grid, col_c).unwrap().node_j_id;
        let gir_ab = grid.girder_between(node_a, node_b).unwrap();
        let gir_ac = grid.girder_between(node_a, node_c).unwrap();
        let far_stable_ids: HashSet<i32> = [col_a, col_b, col_c, gir_ab, gir_ac].into_iter().collect();

        let col_d = grid.column_starting_at(3, 3, 0).unwrap();
        let col_e = grid.column_starting_at(2, 3, 0).unwrap();
        let col_f = grid.column_starting_at(3, 2, 0).unwrap();
        let node_d = get_element(&grid, col_d).unwrap().node_j_id;
        let node_e = get_element(&grid, col_e).unwrap().node_j_id;
        let node_f = get_element(&grid, col_f).unwrap().node_j_id;
        let gir_de = grid.girder_between(node_d, node_e).unwrap();
        let gir_df = grid.girder_between(node_d, node_f).unwrap();

        let disconnected_bootstrap = vec![col_d, col_e, gir_de, col_f, gir_df];
        assert!(
            check_bundle_stability(&disconnected_bootstrap, &grid, &far_stable_ids),
            "disconnected local bootstrap should still pass even when unrelated stable structure exists elsewhere"
        );
    }

    #[test]
    fn test_local_context_allows_connected_extension_without_global_recheck() {
        let grid = SimGrid::new(4, 4, 2, 6000.0, 6000.0, 4000.0);

        let col_a = grid.column_starting_at(0, 0, 0).unwrap();
        let col_b = grid.column_starting_at(1, 0, 0).unwrap();
        let col_c = grid.column_starting_at(0, 1, 0).unwrap();
        let node_a = get_element(&grid, col_a).unwrap().node_j_id;
        let node_b = get_element(&grid, col_b).unwrap().node_j_id;
        let node_c = get_element(&grid, col_c).unwrap().node_j_id;
        let gir_ab = grid.girder_between(node_a, node_b).unwrap();
        let gir_ac = grid.girder_between(node_a, node_c).unwrap();
        let stable_ids: HashSet<i32> = [col_a, col_b, col_c, gir_ab, gir_ac].into_iter().collect();

        let new_col = grid.column_starting_at(1, 1, 0).unwrap();
        let new_node = get_element(&grid, new_col).unwrap().node_j_id;
        let ext_girder = grid.girder_between(node_b, new_node).unwrap();
        let extension = vec![new_col, ext_girder];

        assert!(
            check_bundle_stability(&extension, &grid, &stable_ids),
            "extension should pass when adjacent local stable structure provides the needed support/context"
        );
    }

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

    #[test]
    fn test_run_scenario_default_grid_4x4x3() {
        let grid = SimGrid::new(4, 4, 3, 6000.0, 6000.0, 4000.0);
        let wfs = vec![SimWorkfront {
            id: 1,
            grid_x: 0,
            grid_y: 0,
        }];
        let t0 = std::time::Instant::now();
        let scenario = run_scenario(1, &grid, &wfs, 42, (0.5, 0.3, 0.2), 0.3);
        let elapsed = t0.elapsed();
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

    #[test]
    fn test_upper_floor_blocked_when_lower_nearly_complete() {
        // Test: If lower floor has 5 or fewer uninstalled columns,
        // upper floor installation should be blocked.
        let grid = SimGrid::new(3, 3, 2, 6000.0, 6000.0, 4000.0);
        // 3x3 grid = 9 columns per floor
        // Install 5 columns on floor 1 (4 remaining <= 5) -> upper should be blocked

        let floor1_cols: Vec<i32> = grid
            .elements
            .iter()
            .filter(|e| {
                e.member_type == "Column"
                    && grid
                        .node_coords(e.node_i_id)
                        .map(|(_, _, z)| z.abs() < 0.001)
                        .unwrap_or(false)
            })
            .take(5)
            .map(|e| e.id)
            .collect();

        let installed: HashSet<i32> = floor1_cols.into_iter().collect();
        // 5 installed, 4 remaining -> should block upper floor

        // Find a floor 2 column
        let dz = grid_dz(&grid);
        let floor2_col = grid
            .elements
            .iter()
            .find(|e| {
                e.member_type == "Column"
                    && grid
                        .node_coords(e.node_i_id)
                        .map(|(_, _, z)| {
                            let floor = (z / dz).round() as i32 + 1;
                            floor == 2
                        })
                        .unwrap_or(false)
            })
            .map(|e| e.id);

        if let Some(col2_id) = floor2_col {
            // With threshold = 0.5, upper floor should be blocked when 4 remaining
            let allowed = check_upper_floor_constraint(&[col2_id], &grid, &installed, 0.5, 0.8, 5);
            assert!(
                !allowed,
                "Upper floor should be blocked when lower has 4 remaining columns (<=5)"
            );
        }
    }

    #[test]
    fn test_upper_floor_allowed_when_lower_complete() {
        // Test: If lower floor is 100% complete, upper floor should be allowed
        let grid = SimGrid::new(2, 2, 2, 6000.0, 6000.0, 4000.0);
        let dz = grid_dz(&grid);

        // Install ALL floor 1 columns
        let floor1_cols: Vec<i32> = grid
            .elements
            .iter()
            .filter(|e| {
                e.member_type == "Column"
                    && grid
                        .node_coords(e.node_i_id)
                        .map(|(_, _, z)| z.abs() < 0.001)
                        .unwrap_or(false)
            })
            .map(|e| e.id)
            .collect();

        let installed: HashSet<i32> = floor1_cols.into_iter().collect();

        // Find a floor 2 column
        let floor2_col = grid
            .elements
            .iter()
            .find(|e| {
                e.member_type == "Column"
                    && grid
                        .node_coords(e.node_i_id)
                        .map(|(_, _, z)| {
                            let floor = (z / dz).round() as i32 + 1;
                            floor == 2
                        })
                        .unwrap_or(false)
            })
            .map(|e| e.id);

        if let Some(col2_id) = floor2_col {
            let allowed = check_upper_floor_constraint(&[col2_id], &grid, &installed, 0.3, 0.8, 5);
            assert!(
                allowed,
                "Upper floor should be allowed when lower is 100% complete"
            );
        }
    }

    #[test]
    fn test_upper_floor_constraint_tracked_single_column_rate_based() {
        // Use 4 z-levels so floor 2 is not the top floor (B bypass should not apply).
        let grid = SimGrid::new(4, 4, 4, 6000.0, 6000.0, 4000.0);
        let dz = grid_dz(&grid);
        let tracker = FloorTracker::from_grid(&grid, dz);

        let floor1_cols: Vec<i32> = grid
            .elements
            .iter()
            .filter(|e| {
                e.member_type == "Column"
                    && grid
                        .node_coords(e.node_i_id)
                        .map(|(_, _, z)| z.abs() < 0.001)
                        .unwrap_or(false)
            })
            .map(|e| e.id)
            .collect();

        let floor2_col = grid
            .elements
            .iter()
            .find(|e| {
                e.member_type == "Column"
                    && grid
                        .node_coords(e.node_i_id)
                        .map(|(_, _, z)| ((z / dz).round() as i32 + 1) == 2)
                        .unwrap_or(false)
            })
            .map(|e| e.id)
            .expect("floor2 column should exist");

        // Keep this test in the pure ratio-governed zone
        // (before forced-completion guard kicks in and before C-threshold relaxes).
        for installed_count in 0..=10 {
            let installed: HashSet<i32> = floor1_cols
                .iter()
                .take(installed_count)
                .copied()
                .collect();
            let installed_per_floor = tracker.installed_per_floor_from(&installed);

            let tracked = check_upper_floor_constraint_tracked(
                &[floor2_col],
                &tracker,
                &installed_per_floor,
                0.3,
                0.8,
                5,
            );

            // With one new upper-floor column candidate:
            // ratio = 1 / installed_lower. Allow when ratio <= 0.3.
            // installed_lower == 0 is treated as pass in tracked logic.
            let expected = if installed_count == 0 {
                true
            } else {
                (1.0 / installed_count as f64) <= 0.3
            };

            assert_eq!(
                expected, tracked,
                "mismatch at installed_count={} for floor2_col={}",
                installed_count, floor2_col
            );
        }
    }

    #[test]
    fn test_ab_upper_floor_constraint_legacy_vs_tracked_multiple_candidates() {
        let grid = SimGrid::new(4, 4, 3, 6000.0, 6000.0, 4000.0);
        let dz = grid_dz(&grid);
        let tracker = FloorTracker::from_grid(&grid, dz);

        let floor1_cols: Vec<i32> = grid
            .elements
            .iter()
            .filter(|e| {
                e.member_type == "Column"
                    && grid
                        .node_coords(e.node_i_id)
                        .map(|(_, _, z)| z.abs() < 0.001)
                        .unwrap_or(false)
            })
            .map(|e| e.id)
            .collect();

        let floor2_cols: Vec<i32> = grid
            .elements
            .iter()
            .filter(|e| {
                e.member_type == "Column"
                    && grid
                        .node_coords(e.node_i_id)
                        .map(|(_, _, z)| ((z / dz).round() as i32 + 1) == 2)
                        .unwrap_or(false)
            })
            .take(3)
            .map(|e| e.id)
            .collect();

        let installed: HashSet<i32> = floor1_cols.iter().take(7).copied().collect();
        let installed_per_floor = tracker.installed_per_floor_from(&installed);

        let legacy = check_upper_floor_constraint_legacy(&floor2_cols, &grid, &installed, 0.3);
        let tracked =
            check_upper_floor_constraint_tracked(&floor2_cols, &tracker, &installed_per_floor, 0.3, 0.8, 5);

        assert_eq!(legacy, tracked);
    }

    #[test]
    fn test_ab_collect_single_candidates_legacy_vs_optimized_no_priority() {
        let grid = SimGrid::new(4, 4, 3, 6000.0, 6000.0, 4000.0);
        let wf = SimWorkfront {
            id: 1,
            grid_x: 1,
            grid_y: 1,
        };
        let node_pos = build_node_pos(&grid);

        let support_ids: HashSet<i32> = grid
            .elements
            .iter()
            .filter(|e| e.member_type == "Column")
            .take(6)
            .map(|e| e.id)
            .collect();

        let local_element_ids: HashSet<i32> = support_ids.iter().take(3).copied().collect();
        let committed_ids: HashSet<i32> = support_ids.iter().take(4).copied().collect();

        let legacy = collect_single_candidates_legacy(
            &wf,
            &grid,
            &support_ids,
            &local_element_ids,
            &committed_ids,
            &node_pos,
        );
        let optimized = collect_single_candidates_optimized(
            &wf,
            &grid,
            &support_ids,
            &local_element_ids,
            &committed_ids,
            &node_pos,
        );

        let legacy_sig: Vec<(i32, usize, i32)> = legacy
            .iter()
            .map(|c| {
                (
                    c.element_id,
                    c.connectivity,
                    (c.frontier_dist * 1000.0).round() as i32,
                )
            })
            .collect();
        let optimized_sig: Vec<(i32, usize, i32)> = optimized
            .iter()
            .map(|c| {
                (
                    c.element_id,
                    c.connectivity,
                    (c.frontier_dist * 1000.0).round() as i32,
                )
            })
            .collect();

        assert_eq!(legacy_sig, optimized_sig);
    }

    #[test]
    fn test_ab_collect_single_candidates_legacy_vs_optimized_with_priority_floor() {
        let grid = SimGrid::new(4, 4, 3, 6000.0, 6000.0, 4000.0);
        let wf = SimWorkfront {
            id: 1,
            grid_x: 0,
            grid_y: 0,
        };
        let node_pos = build_node_pos(&grid);

        let support_ids: HashSet<i32> = grid
            .elements
            .iter()
            .filter(|e| e.member_type == "Column")
            .take(10)
            .map(|e| e.id)
            .collect();

        let local_element_ids: HashSet<i32> = support_ids.iter().take(2).copied().collect();
        let committed_ids: HashSet<i32> = support_ids.iter().take(5).copied().collect();

        let legacy = collect_single_candidates_legacy(
            &wf,
            &grid,
            &support_ids,
            &local_element_ids,
            &committed_ids,
            &node_pos,
        );
        let optimized = collect_single_candidates_optimized(
            &wf,
            &grid,
            &support_ids,
            &local_element_ids,
            &committed_ids,
            &node_pos,
        );

        let legacy_sig: Vec<(i32, usize, i32)> = legacy
            .iter()
            .map(|c| {
                (
                    c.element_id,
                    c.connectivity,
                    (c.frontier_dist * 1000.0).round() as i32,
                )
            })
            .collect();
        let optimized_sig: Vec<(i32, usize, i32)> = optimized
            .iter()
            .map(|c| {
                (
                    c.element_id,
                    c.connectivity,
                    (c.frontier_dist * 1000.0).round() as i32,
                )
            })
            .collect();

        assert_eq!(legacy_sig, optimized_sig);
    }

    #[test]
    fn test_upper_floor_constraint_bypassed_on_top_floor() {
        let grid = SimGrid::new(4, 4, 3, 6000.0, 6000.0, 4000.0);
        let dz = grid_dz(&grid);
        let tracker = FloorTracker::from_grid(&grid, dz);

        let top_floor = tracker.max_floor;
        let lower_floor = (top_floor - 1).max(1);

        let lower_floor_cols: Vec<i32> = grid
            .elements
            .iter()
            .filter(|e| {
                e.member_type == "Column"
                    && grid
                        .node_coords(e.node_i_id)
                        .map(|(_, _, z)| ((z / dz).round() as i32 + 1) == lower_floor)
                        .unwrap_or(false)
            })
            .map(|e| e.id)
            .collect();

        let top_floor_col = grid
            .elements
            .iter()
            .find(|e| {
                e.member_type == "Column"
                    && grid
                        .node_coords(e.node_i_id)
                        .map(|(_, _, z)| ((z / dz).round() as i32 + 1) == top_floor)
                        .unwrap_or(false)
            })
            .map(|e| e.id)
            .expect("top-floor column should exist");

        // Intentionally low lower-floor progress. With B rule, top floor should bypass ratio gate.
        let installed: HashSet<i32> = lower_floor_cols.iter().take(2).copied().collect();
        let installed_per_floor = tracker.installed_per_floor_from(&installed);

        let allowed = check_upper_floor_constraint_tracked(
            &[top_floor_col],
            &tracker,
            &installed_per_floor,
            0.3,
            0.8,
            5,
        );

        assert!(allowed, "top-floor column should bypass upper-floor ratio constraint");
    }

    #[test]
    fn test_upper_floor_constraint_relaxed_when_lower_floor_reaches_threshold() {
        // Use a larger floor so 80% completion can be reached while
        // still leaving more than 5 columns (forced-completion guard not active).
        // Also use 4 z-levels so floor 2 is not top floor.
        let grid = SimGrid::new(6, 6, 4, 6000.0, 6000.0, 4000.0);
        let dz = grid_dz(&grid);
        let tracker = FloorTracker::from_grid(&grid, dz);

        let floor1_cols: Vec<i32> = grid
            .elements
            .iter()
            .filter(|e| {
                e.member_type == "Column"
                    && grid
                        .node_coords(e.node_i_id)
                        .map(|(_, _, z)| ((z / dz).round() as i32 + 1) == 1)
                        .unwrap_or(false)
            })
            .map(|e| e.id)
            .collect();

        let floor2_col = grid
            .elements
            .iter()
            .find(|e| {
                e.member_type == "Column"
                    && grid
                        .node_coords(e.node_i_id)
                        .map(|(_, _, z)| ((z / dz).round() as i32 + 1) == 2)
                        .unwrap_or(false)
            })
            .map(|e| e.id)
            .expect("floor2 column should exist");

        // Force ratio failure if checked: installed_upper=1, installed_lower=29 => 0.034 > 0.01
        // C rule should bypass ratio because lower floor completion is already 80%+ (29/36)
        // and remaining columns are still > 5 (so forced-completion guard is not active).
        let installed: HashSet<i32> = floor1_cols.iter().take(29).copied().collect();
        let installed_per_floor = tracker.installed_per_floor_from(&installed);

        let allowed = check_upper_floor_constraint_tracked(
            &[floor2_col],
            &tracker,
            &installed_per_floor,
            0.01,
            0.8,
            5,
        );

        assert!(
            allowed,
            "upper-floor ratio gate should be relaxed once lower floor completion reaches threshold"
        );
    }

    #[test]
    fn test_ground_column_is_structurally_stable() {
        // Test: A ground-level column (z=0 base) is always structurally stable
        // (connectivity/proximity is handled by candidate generation, not stability check)
        let grid = SimGrid::new(4, 4, 2, 6000.0, 6000.0, 4000.0);

        // Get any ground-level column
        let corner_col = grid.column_starting_at(3, 3, 0);
        let installed: HashSet<i32> = HashSet::new();

        if let Some(col_id) = corner_col {
            let stable = check_single_stability(col_id, &grid, &installed);
            assert!(
                stable,
                "Ground-level column should be structurally stable (z=0 base)"
            );
        }
    }

    #[test]
    fn test_floor_eligibility_uses_column_completion_ratio() {
        let grid = SimGrid::new(4, 4, 3, 6000.0, 6000.0, 4000.0);
        let dz = grid_dz(&grid);
        let tracker = FloorTracker::from_grid(&grid, dz);

        let floor1_cols: Vec<i32> = grid
            .elements
            .iter()
            .filter(|e| {
                e.member_type == "Column"
                    && grid
                        .node_coords(e.node_i_id)
                        .map(|(_, _, z)| ((z / dz).round() as i32 + 1) == 1)
                        .unwrap_or(false)
            })
            .map(|e| e.id)
            .collect();

        let installed: HashSet<i32> = floor1_cols.iter().copied().collect();
        let installed_columns_per_floor = tracker.installed_per_floor_from(&installed);
        let constraints = SimConstraints {
            upper_floor_column_rate_threshold: 0.3,
            lower_floor_completion_ratio_threshold: 0.8,
            lower_floor_forced_completion_threshold: 5,
        };

        assert!(is_floor_eligible_for_new_work(
            2,
            &installed_columns_per_floor,
            &tracker.total_per_floor,
            &constraints,
        ));
    }

    #[test]
    fn test_choose_target_floor_prefers_upper_deficit_then_returns_lower() {
        let constraints = SimConstraints {
            upper_floor_column_rate_threshold: 0.3,
            lower_floor_completion_ratio_threshold: 0.8,
            lower_floor_forced_completion_threshold: 5,
        };
        let candidate_floors = vec![1, 2];

        let mut installed_columns_per_floor = HashMap::new();
        installed_columns_per_floor.insert(1, 10);
        installed_columns_per_floor.insert(2, 0);

        let upper_first = choose_target_floor(
            &candidate_floors,
            &installed_columns_per_floor,
            &constraints,
            None,
        );
        assert_eq!(upper_first, 2, "upper floor should be chosen while it is below target ratio");

        installed_columns_per_floor.insert(2, 3);
        let lower_next = choose_target_floor(
            &candidate_floors,
            &installed_columns_per_floor,
            &constraints,
            None,
        );
        assert_eq!(lower_next, 1, "once upper catches up to target ratio, lower floor should resume");
    }

    #[test]
    fn test_floor_sequence_returns_to_lower_after_upper_start() {
        let grid = SimGrid::new(4, 4, 3, 6000.0, 6000.0, 4000.0);
        let wfs = vec![SimWorkfront {
            id: 1,
            grid_x: 1,
            grid_y: 1,
        }];

        let constraints = SimConstraints {
            upper_floor_column_rate_threshold: 0.3,
            lower_floor_completion_ratio_threshold: 0.8,
            lower_floor_forced_completion_threshold: 5,
        };

        let scenario = run_scenario_internal(
            1,
            &grid,
            &wfs,
            777,
            (0.5, 0.3, 0.2),
            constraints,
            None,
        );

        let first_upper_idx = scenario.steps.iter().position(|step| step.floor > 1);
        let Some(first_upper_idx) = first_upper_idx else {
            panic!("expected an upper-floor step");
        };

        let returns_to_lower = scenario
            .steps
            .iter()
            .skip(first_upper_idx + 1)
            .any(|step| step.floor == 1);

        assert!(
            returns_to_lower,
            "after upper-floor work starts, scheduler should later return to lower floor for interleaving"
        );
    }

    // ============================================================
    // SIMULATION COMPLETION TESTS - Critical performance tests
    // ============================================================

    #[test]
    fn test_simulation_completes_2x2x2_all_elements() {
        // Small grid: 2x2x2 should complete ALL elements
        let grid = SimGrid::new(2, 2, 2, 6000.0, 6000.0, 4000.0);
        let wfs = vec![SimWorkfront {
            id: 1,
            grid_x: 0,
            grid_y: 0,
        }];

        let scenario = run_scenario(1, &grid, &wfs, 42, (0.5, 0.3, 0.2), 0.5);
        let total_elements = grid.elements.len();
        let installed: usize = scenario.steps.iter().map(|s| s.element_ids.len()).sum();

        assert_eq!(
            installed, total_elements,
            "2x2x2 grid should install ALL {} elements, got {}. Termination: {:?}",
            total_elements, installed, scenario.metrics.termination_reason
        );
        assert_eq!(
            scenario.metrics.termination_reason,
            TerminationReason::Completed,
            "Should terminate with Completed, not {:?}",
            scenario.metrics.termination_reason
        );
    }

    #[test]
    fn test_upper_floor_step_can_start_before_lower_floor_complete() {
        let grid = SimGrid::new(4, 4, 3, 6000.0, 6000.0, 4000.0);
        let wfs = vec![SimWorkfront {
            id: 1,
            grid_x: 1,
            grid_y: 1,
        }];

        let constraints = SimConstraints {
            upper_floor_column_rate_threshold: 0.3,
            lower_floor_completion_ratio_threshold: 0.8,
            lower_floor_forced_completion_threshold: 5,
        };

        let scenario = run_scenario_internal(
            1,
            &grid,
            &wfs,
            777,
            (0.5, 0.3, 0.2),
            constraints,
            None,
        );

        let dz = grid_dz(&grid);
        let total_floor1_elements = grid
            .elements
            .iter()
            .filter(|e| element_floor(e.id, &grid, dz) == Some(1))
            .count();

        let mut installed_floor1 = 0usize;
        let mut found_upper_before_floor1_complete = false;

        for step in &scenario.steps {
            let has_floor2_or_above = step
                .element_ids
                .iter()
                .any(|eid| element_floor(*eid, &grid, dz).unwrap_or(1) > 1);

            if has_floor2_or_above {
                found_upper_before_floor1_complete = installed_floor1 < total_floor1_elements;
                break;
            }

            installed_floor1 += step
                .element_ids
                .iter()
                .filter(|eid| element_floor(**eid, &grid, dz) == Some(1))
                .count();
        }

        assert!(
            found_upper_before_floor1_complete,
            "Expected upper-floor step to appear before floor 1 is fully complete"
        );
    }

    #[test]
    fn test_simulation_completes_3x3x2_all_elements() {
        // Medium grid: 3x3x2
        let grid = SimGrid::new(3, 3, 2, 6000.0, 6000.0, 4000.0);
        let wfs = vec![SimWorkfront {
            id: 1,
            grid_x: 1,
            grid_y: 1,
        }];

        let scenario = run_scenario(1, &grid, &wfs, 123, (0.5, 0.3, 0.2), 0.5);
        let total_elements = grid.elements.len();
        let installed: usize = scenario.steps.iter().map(|s| s.element_ids.len()).sum();

        assert_eq!(
            installed, total_elements,
            "3x3x2 grid should install ALL {} elements, got {}. Termination: {:?}",
            total_elements, installed, scenario.metrics.termination_reason
        );
    }

    #[test]
    fn test_simulation_completes_4x4x3_all_elements() {
        // Larger grid: 4x4x3 (the default mentioned by user)
        let grid = SimGrid::new(4, 4, 3, 6000.0, 6000.0, 4000.0);
        let wfs = vec![SimWorkfront {
            id: 1,
            grid_x: 1,
            grid_y: 1,
        }];

        let t0 = std::time::Instant::now();
        let scenario = run_scenario(1, &grid, &wfs, 777, (0.5, 0.3, 0.2), 0.5);
        let elapsed = t0.elapsed();

        let total_elements = grid.elements.len();
        let installed: usize = scenario.steps.iter().map(|s| s.element_ids.len()).sum();

        assert_eq!(
            installed, total_elements,
            "4x4x3 grid should install ALL {} elements, got {}. Termination: {:?}",
            total_elements, installed, scenario.metrics.termination_reason
        );
        assert!(
            elapsed.as_secs() < 30,
            "4x4x3 simulation should complete in <30s, took {:?}",
            elapsed
        );
    }

    #[test]
    fn test_simulation_completes_5x5x4_all_elements() {
        // Large grid stress test: 5x5x4
        let grid = SimGrid::new(5, 5, 4, 6000.0, 6000.0, 4000.0);
        let wfs = vec![SimWorkfront {
            id: 1,
            grid_x: 2,
            grid_y: 2,
        }];

        let t0 = std::time::Instant::now();
        let scenario = run_scenario(1, &grid, &wfs, 999, (0.5, 0.3, 0.2), 0.5);
        let elapsed = t0.elapsed();

        let total_elements = grid.elements.len();
        let installed: usize = scenario.steps.iter().map(|s| s.element_ids.len()).sum();

        assert_eq!(
            installed, total_elements,
            "5x5x4 grid should install ALL {} elements, got {}. Termination: {:?}",
            total_elements, installed, scenario.metrics.termination_reason
        );
        assert!(
            elapsed.as_secs() < 60,
            "5x5x4 simulation should complete in <60s, took {:?}",
            elapsed
        );
    }

    #[test]
    fn test_simulation_completes_4x8x3_multi_workfront_all_elements() {
        let grid = SimGrid::new(4, 8, 3, 6000.0, 6000.0, 4000.0);
        let wfs = vec![
            SimWorkfront { id: 1, grid_x: 0, grid_y: 0 },
            SimWorkfront { id: 2, grid_x: 3, grid_y: 0 },
            SimWorkfront { id: 3, grid_x: 0, grid_y: 7 },
            SimWorkfront { id: 4, grid_x: 3, grid_y: 7 },
            SimWorkfront { id: 5, grid_x: 1, grid_y: 3 },
        ];

        let scenario = run_scenario_internal(
            1,
            &grid,
            &wfs,
            777,
            (0.5, 0.3, 0.2),
            SimConstraints {
                upper_floor_column_rate_threshold: 0.3,
                lower_floor_completion_ratio_threshold: 0.8,
                lower_floor_forced_completion_threshold: 10,
            },
            None,
        );

        let total_elements = grid.elements.len();
        let installed_ids: HashSet<i32> = scenario
            .steps
            .iter()
            .flat_map(|s| s.element_ids.iter().copied())
            .collect();
        let installed = installed_ids.len();
        let missing: Vec<String> = grid
            .elements
            .iter()
            .filter(|e| !installed_ids.contains(&e.id))
            .map(|e| {
                let floor = element_floor(e.id, &grid, grid_dz(&grid)).unwrap_or(-1);
                format!("{}:{}:F{}", e.id, e.member_type, floor)
            })
            .collect();

        assert_eq!(
            installed, total_elements,
            "4x8x3 multi-workfront grid should install ALL {} elements, got {}. Termination: {:?}. Missing: {:?}",
            total_elements, installed, scenario.metrics.termination_reason, missing
        );
    }

    #[test]
    fn test_simulation_multiple_workfronts_completes() {
        // Multiple workfronts should also complete all elements
        let grid = SimGrid::new(4, 4, 2, 6000.0, 6000.0, 4000.0);
        let wfs = vec![
            SimWorkfront {
                id: 1,
                grid_x: 0,
                grid_y: 0,
            },
            SimWorkfront {
                id: 2,
                grid_x: 3,
                grid_y: 3,
            },
        ];

        let scenario = run_scenario(1, &grid, &wfs, 42, (0.5, 0.3, 0.2), 0.5);
        let total_elements = grid.elements.len();
        let installed: usize = scenario.steps.iter().map(|s| s.element_ids.len()).sum();

        assert_eq!(
            installed, total_elements,
            "4x4x2 with 2 workfronts should install ALL {} elements, got {}. Termination: {:?}",
            total_elements, installed, scenario.metrics.termination_reason
        );
    }

    #[test]
    fn test_simulation_proceeds_beyond_bootstrap() {
        // Critical: Simulation MUST continue after bootstrap (step 1)
        let grid = SimGrid::new(3, 3, 2, 6000.0, 6000.0, 4000.0);
        let wfs = vec![SimWorkfront {
            id: 1,
            grid_x: 1,
            grid_y: 1,
        }];

        let scenario = run_scenario(1, &grid, &wfs, 42, (0.5, 0.3, 0.2), 0.5);

        // Bootstrap = 1 step with 5 elements (3 cols + 2 girders)
        // If simulation stops at bootstrap, total_steps would be 1
        // A 3x3x2 grid has many more elements, so we need many more steps
        assert!(
            scenario.metrics.total_steps > 1,
            "Simulation MUST proceed beyond bootstrap. Got {} steps. Termination: {:?}",
            scenario.metrics.total_steps,
            scenario.metrics.termination_reason
        );

        let total_elements = grid.elements.len();
        let installed: usize = scenario.steps.iter().map(|s| s.element_ids.len()).sum();
        assert!(
            installed > 5,
            "Must install more than bootstrap (5 elements). Got {}",
            installed
        );
        assert_eq!(installed, total_elements, "Should complete all elements");
    }

    #[test]
    fn test_simulation_no_duplicate_installations() {
        // Each element should be installed exactly once
        let grid = SimGrid::new(3, 3, 2, 6000.0, 6000.0, 4000.0);
        let wfs = vec![SimWorkfront {
            id: 1,
            grid_x: 1,
            grid_y: 1,
        }];

        let scenario = run_scenario(1, &grid, &wfs, 42, (0.5, 0.3, 0.2), 0.5);

        let mut all_installed: Vec<i32> = scenario
            .steps
            .iter()
            .flat_map(|s| s.element_ids.iter().copied())
            .collect();
        let before_dedup = all_installed.len();
        all_installed.sort();
        all_installed.dedup();
        let after_dedup = all_installed.len();

        assert_eq!(
            before_dedup, after_dedup,
            "No element should be installed twice. {} before dedup, {} after",
            before_dedup, after_dedup
        );
    }

    #[test]
    fn test_parallel_scenarios_all_complete() {
        // Multiple scenarios in parallel should all complete
        let grid = SimGrid::new(3, 3, 2, 6000.0, 6000.0, 4000.0);
        let wfs = vec![SimWorkfront {
            id: 1,
            grid_x: 1,
            grid_y: 1,
        }];

        let scenarios = run_all_scenarios(5, &grid, &wfs, (0.5, 0.3, 0.2), 0.5);
        let total_elements = grid.elements.len();

        for scenario in &scenarios {
            let installed: usize = scenario.steps.iter().map(|s| s.element_ids.len()).sum();
            assert_eq!(
                installed, total_elements,
                "Scenario {} should complete all {} elements, got {}. Termination: {:?}",
                scenario.id, total_elements, installed, scenario.metrics.termination_reason
            );
        }
    }

    #[test]
    fn test_local_steps_merged_into_single_global_step() {
        // Multi-workfront scenario: each global step should collect local steps from workfronts
        let grid = SimGrid::new(4, 4, 2, 6000.0, 6000.0, 4000.0);
        let wfs = vec![
            SimWorkfront { id: 1, grid_x: 0, grid_y: 0 },
            SimWorkfront { id: 2, grid_x: 3, grid_y: 3 },
        ];

        let scenario = run_scenario(1, &grid, &wfs, 42, (0.5, 0.3, 0.2), 0.5);
        assert!(
            matches!(scenario.metrics.termination_reason, TerminationReason::Completed),
            "Scenario should complete"
        );

        // At least some steps should have multiple local_steps (multi-workfront merge)
        let multi_wf_steps = scenario.steps.iter()
            .filter(|s| s.local_steps.len() > 1)
            .count();
        assert!(
            multi_wf_steps > 0,
            "With 2 workfronts, at least some steps should contain local_steps from multiple workfronts"
        );

        // Each local_step should have non-empty element_ids and valid workfront_id
        for step in &scenario.steps {
            for ls in &step.local_steps {
                assert!(!ls.element_ids.is_empty(), "local_step element_ids must not be empty");
                assert!(ls.workfront_id >= 1, "local_step workfront_id must be 1-based");
                assert!(!ls.pattern.is_empty(), "local_step pattern must not be empty");
            }
        }
    }

    #[test]
    fn test_sequence_collation_round_robin() {
        // Verify from_local_steps produces correct round-robin sequence collation
        let grid = SimGrid::new(4, 4, 2, 6000.0, 6000.0, 4000.0);
        let wfs = vec![
            SimWorkfront { id: 1, grid_x: 0, grid_y: 0 },
            SimWorkfront { id: 2, grid_x: 3, grid_y: 3 },
        ];

        let scenario = run_scenario(1, &grid, &wfs, 42, (0.5, 0.3, 0.2), 0.5);

        for step in &scenario.steps {
            if step.local_steps.len() > 1 {
                // In round-robin collation, same round should share the same sequence_number
                let mut seq_nums: Vec<usize> = step.sequences.iter()
                    .map(|s| s.sequence_number)
                    .collect();
                seq_nums.sort();
                seq_nums.dedup();

                // Number of unique sequence numbers should equal sequence_round_count
                let expected_rounds = step.sequence_round_count();
                assert_eq!(
                    seq_nums.len(), expected_rounds,
                    "Unique sequence numbers should match round count: {} vs {}",
                    seq_nums.len(), expected_rounds
                );
                break; // One verified multi-WF step is sufficient
            }
        }
    }

    #[test]
    fn test_successful_workfront_excluded_from_same_cycle() {
        // After completing a local step, a workfront should be excluded from the same global step cycle
        let grid = SimGrid::new(4, 4, 2, 6000.0, 6000.0, 4000.0);
        let wfs = vec![
            SimWorkfront { id: 1, grid_x: 0, grid_y: 0 },
            SimWorkfront { id: 2, grid_x: 3, grid_y: 3 },
        ];

        let scenario = run_scenario(1, &grid, &wfs, 42, (0.5, 0.3, 0.2), 0.5);

        for step in &scenario.steps {
            // Each workfront should appear at most once in local_steps of a single global step
            let mut seen_wf: std::collections::HashSet<i32> = std::collections::HashSet::new();
            for ls in &step.local_steps {
                assert!(
                    seen_wf.insert(ls.workfront_id),
                    "Workfront {} appears multiple times in same global step — should be excluded after first completion",
                    ls.workfront_id
                );
            }
        }
    }

    #[test]
    fn test_global_step_element_ids_match_local_steps_union() {
        // step.element_ids should be the union of all local_steps' element_ids
        let grid = SimGrid::new(4, 4, 2, 6000.0, 6000.0, 4000.0);
        let wfs = vec![
            SimWorkfront { id: 1, grid_x: 0, grid_y: 0 },
            SimWorkfront { id: 2, grid_x: 3, grid_y: 3 },
        ];

        let scenario = run_scenario(1, &grid, &wfs, 42, (0.5, 0.3, 0.2), 0.5);

        for (i, step) in scenario.steps.iter().enumerate() {
            let mut local_ids: Vec<i32> = step.local_steps.iter()
                .flat_map(|ls| ls.element_ids.iter().copied())
                .collect();
            local_ids.sort();
            let mut step_ids = step.element_ids.clone();
            step_ids.sort();
            assert_eq!(
                step_ids, local_ids,
                "Step {} element_ids should match union of local_steps element_ids",
                i + 1
            );
        }
    }
}
