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
use crate::sim_trace::{
    format_ids, SimulationTraceConfig, SimulationTraceEvent, SimulationTraceLevel,
    SimulationTraceLogger, SimulationTraceRunContext, SimulationTraceVerbosity,
};
use crate::stability::{
    check_step_bundle_stability, classify_member_signature, StepBufferDecision,
    StepCandidateMask, StepPatternType, StabilityElement,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmitResult {
    /// Local step successfully emitted
    Emitted,
    /// Buffer retained — waiting for a larger pattern
    Deferred,
    /// Stability check failed — buffer must be rolled back
    Infeasible,
}

#[derive(Clone, Copy, Debug)]
pub struct SimConstraints {
    pub upper_floor_column_rate_threshold: f64,
    pub lower_floor_completion_ratio_threshold: f64,
}

#[derive(Default, Clone)]
struct WorkfrontState {
    owned_ids: HashSet<i32>,
    buffer_sequences: Vec<SimSequence>,
    committed_floor: Option<i32>,
}

impl WorkfrontState {
    fn all_local_ids(&self) -> HashSet<i32> {
        let mut ids = self.owned_ids.clone();
        ids.extend(self.buffer_sequences.iter().map(|seq| seq.element_id));
        ids
    }

    fn buffer_local_ids(&self) -> HashSet<i32> {
        self.buffer_sequences
            .iter()
            .map(|seq| seq.element_id)
            .collect()
    }

    fn buffer_element_ids(&self) -> Vec<i32> {
        self.buffer_sequences
            .iter()
            .map(|seq| seq.element_id)
            .collect()
    }
}


#[derive(Clone)]
struct SingleCandidate {
    element_id: i32,
    connectivity: usize,
    frontier_dist: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CandidateAdvanceResult {
    Accepted,
    RejectedInvalidPattern,
    RejectedUnstableComplete,
}

impl CandidateAdvanceResult {
    fn accepted(self) -> bool {
        matches!(self, Self::Accepted)
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::RejectedInvalidPattern => "invalid_pattern",
            Self::RejectedUnstableComplete => "unstable_complete",
        }
    }
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

fn compute_cycle_committed_ids(
    stable_ids: &HashSet<i32>,
    workfront_states: &HashMap<i32, WorkfrontState>,
    cycle_local_steps: &[LocalStep],
) -> HashSet<i32> {
    let mut committed_ids: HashSet<i32> = workfront_states.values().fold(stable_ids.clone(), |mut acc, state| {
        acc.extend(state.buffer_sequences.iter().map(|seq| seq.element_id));
        acc
    });

    for local_step in cycle_local_steps {
        committed_ids.extend(local_step.element_ids.iter().copied());
    }

    committed_ids
}

fn build_cycle_stable_context(
    stable_ids: &HashSet<i32>,
    cycle_local_steps: &[LocalStep],
) -> HashSet<i32> {
    let mut cycle_stable_context: HashSet<i32> = stable_ids.clone();
    for local_step in cycle_local_steps {
        cycle_stable_context.extend(local_step.element_ids.iter().copied());
    }
    cycle_stable_context
}

fn try_emit_completed_buffer(
    wf_id: i32,
    state: &mut WorkfrontState,
    pattern: &StepPatternType,
    grid: &SimGrid,
    dz: f64,
    cycle_stable_context: &HashSet<i32>,
    cycle_local_steps: &mut Vec<LocalStep>,
    cycle_completed_wf: &mut HashSet<i32>,
    trace_logger: &mut Option<SimulationTraceLogger>,
    scene_id: usize,
    cycle_index: usize,
    round_index: usize,
) -> EmitResult {
    let buffer_element_ids = state.buffer_element_ids();

    if !check_bundle_stability(&buffer_element_ids, grid, cycle_stable_context) {
        trace_event(
            trace_logger,
            SimulationTraceLevel::Warning,
            "sim.wf.emission_stability_failed",
            Some(scene_id),
            Some(cycle_index),
            Some(round_index),
            Some(wf_id),
            "buffer stability check failed — infeasible",
            vec![
                ("pattern".to_string(), pattern.as_str().to_string()),
                ("buffer".to_string(), format_ids(buffer_element_ids.iter().copied())),
            ],
        );
        return EmitResult::Infeasible;
    }

    let step_floor = buffer_element_ids
        .iter()
        .filter_map(|eid| element_floor(*eid, grid, dz))
        .min()
        .unwrap_or(1);

    cycle_local_steps.push(LocalStep {
        workfront_id: wf_id,
        element_ids: buffer_element_ids.clone(),
        floor: step_floor,
        pattern: pattern.as_str().to_string(),
    });

    trace_event(
        trace_logger,
        SimulationTraceLevel::Info,
        "sim.wf.local_step_emitted",
        Some(scene_id),
        Some(cycle_index),
        Some(round_index),
        Some(wf_id),
        "local step emitted",
        vec![
            ("pattern".to_string(), pattern.as_str().to_string()),
            ("floor".to_string(), step_floor.to_string()),
            ("element_ids".to_string(), format_ids(buffer_element_ids.iter().copied())),
            (
                "cycle_local_steps_len".to_string(),
                cycle_local_steps.len().to_string(),
            ),
            (
                "cycle_completed_wfs".to_string(),
                format_ids(cycle_completed_wf.iter().copied()),
            ),
        ],
    );

    cycle_completed_wf.insert(wf_id);
    state.buffer_sequences.clear();
    state.committed_floor = None;

    EmitResult::Emitted
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
    anchor: (usize, usize),
) -> f64 {
    if local_positions.is_empty() {
        return candidate_nodes
            .iter()
            .filter_map(|node_id| node_pos.get(node_id))
            .map(|&(xi, yi, _)| {
                ((xi as i32 - anchor.0 as i32).abs() + (yi as i32 - anchor.1 as i32).abs())
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

        let lower_floor_completion_ratio = installed_lower as f64 / total_lower as f64;
        let skip_ratio_gate = floor >= floor_tracker.max_floor
            || lower_floor_completion_ratio >= lower_floor_completion_ratio_threshold;

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

fn candidate_advances_local_step(
    state: &WorkfrontState,
    candidate_id: i32,
    grid: &SimGrid,
    support_ids: &HashSet<i32>,
    has_stable_structure: bool,
) -> CandidateAdvanceResult {
    let mut prospective_buffer = state.buffer_element_ids();
    prospective_buffer.push(candidate_id);

    match classify_buffer(&prospective_buffer, grid, has_stable_structure) {
        StepBufferDecision::Invalid => CandidateAdvanceResult::RejectedInvalidPattern,
        StepBufferDecision::Incomplete(_) => CandidateAdvanceResult::Accepted,
        StepBufferDecision::Complete(_) => {
            if check_bundle_stability(&prospective_buffer, grid, support_ids) {
                CandidateAdvanceResult::Accepted
            } else {
                CandidateAdvanceResult::RejectedUnstableComplete
            }
        }
    }
}

fn locality_seed_ids_for_search(state: &WorkfrontState) -> HashSet<i32> {
    let buffer_local_ids = state.buffer_local_ids();
    if buffer_local_ids.is_empty() {
        state.all_local_ids()
    } else {
        buffer_local_ids
    }
}

fn build_incremental_support_ids(
    stable_ids: &HashSet<i32>,
    cycle_local_steps: &[LocalStep],
    buffer_local_ids: &HashSet<i32>,
) -> HashSet<i32> {
    let mut support_ids = stable_ids.clone();
    for local_step in cycle_local_steps {
        support_ids.extend(local_step.element_ids.iter().copied());
    }
    support_ids.extend(buffer_local_ids.iter().copied());
    support_ids
}

fn candidate_respects_locality(
    elem: &StabilityElement,
    floor: i32,
    anchor: (usize, usize),
    local_positions: &HashSet<(usize, usize)>,
    relax_locality: bool,
    node_pos: &HashMap<i32, (usize, usize, usize)>,
) -> Option<f64> {
    let local_seeded = !local_positions.is_empty();
    let candidate_nodes = [elem.node_i_id, elem.node_j_id];
    let dist = min_xy_distance_to_local_positions(&candidate_nodes, node_pos, local_positions, anchor);

    if !dist.is_finite() {
        return None;
    }

    if !relax_locality && !local_seeded && elem.member_type == "Column" {
        let &(xi, yi, _) = node_pos.get(&elem.node_i_id)?;
        if floor <= 1 {
            if xi != anchor.0 || yi != anchor.1 {
                return None;
            }
        } else if dist > 1.0 {
            return None;
        }
    }

    if !relax_locality && local_seeded && dist > 1.0 && elem.member_type == "Column" {
        return None;
    }

    Some(dist)
}

fn candidate_is_structurally_possible(
    elem: &StabilityElement,
    support_nodes: &HashSet<i32>,
    node_pos: &HashMap<i32, (usize, usize, usize)>,
) -> bool {
    if elem.member_type == "Column" {
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
    }
}

fn try_pick_weighted_candidate_with_retry<F>(
    mut remaining_candidates: Vec<&SingleCandidate>,
    w1: f64,
    w2: f64,
    rng: &mut u64,
    mut validate_candidate: F,
) -> Option<i32>
where
    F: FnMut(&SingleCandidate) -> CandidateAdvanceResult,
{
    while !remaining_candidates.is_empty() {
        let scores: Vec<f64> = remaining_candidates
            .iter()
            .map(|candidate| candidate.score(w1, w2))
            .collect();
        let picked_index = weighted_random_choice(&scores, rng);
        let candidate = remaining_candidates.swap_remove(picked_index);

        if validate_candidate(candidate).accepted() {
            return Some(candidate.element_id);
        }
    }

    None
}

fn try_pick_incremental_candidate(
    state: &WorkfrontState,
    wf: &SimWorkfront,
    anchor: (usize, usize),
    grid: &SimGrid,
    stable_ids: &HashSet<i32>,
    cycle_local_steps: &[LocalStep],
    selected_this_sequence: &HashSet<i32>,
    wf_committed_ids: &HashSet<i32>,
    allowed_floors: &HashSet<i32>,
    floor_tracker: &FloorTracker,
    committed_floor_counts: &HashMap<i32, usize>,
    constraints: &SimConstraints,
    node_pos: &HashMap<i32, (usize, usize, usize)>,
    w1: f64,
    w2: f64,
    rng: &mut u64,
    trace_logger: &mut Option<SimulationTraceLogger>,
    scene_id: usize,
    cycle_index: usize,
    round_index: usize,
) -> Option<i32> {
    let buffer_local_ids = state.buffer_local_ids();
    let locality_seed_ids = locality_seed_ids_for_search(state);

    let support_ids = build_incremental_support_ids(stable_ids, cycle_local_steps, &buffer_local_ids);
    let has_stable = !stable_ids.is_empty();

    let committed_floor = state.committed_floor;
    let buffer_decision = classify_buffer(
        &state.buffer_element_ids(),
        grid,
        has_stable,
    );
    let candidate_mask = match buffer_decision {
        StepBufferDecision::Incomplete(mask) => Some(mask),
        StepBufferDecision::Complete(_) | StepBufferDecision::Invalid => None,
    };

    let wf_candidates = collect_single_candidates(
        wf,
        anchor,
        grid,
        &support_ids,
        &locality_seed_ids,
        wf_committed_ids,
        allowed_floors,
        false,
        node_pos,
    );

    let remaining_candidates: Vec<&SingleCandidate> = wf_candidates
        .iter()
        .filter(|candidate| !selected_this_sequence.contains(&candidate.element_id))
        .filter(|candidate| {
            candidate_mask
                .unwrap_or(StepCandidateMask::Both)
                .allows(is_column(grid, candidate.element_id))
        })
        .filter(|candidate| {
            check_upper_floor_constraint_tracked(
                &[candidate.element_id],
                floor_tracker,
                committed_floor_counts,
                constraints.upper_floor_column_rate_threshold,
                constraints.lower_floor_completion_ratio_threshold,
            )
        })
        .filter(|candidate| {
            let candidate_floor = grid
                .element_floor_by_id
                .get(&candidate.element_id)
                .copied()
                .unwrap_or(1);
            if let Some(locked_floor) = committed_floor {
                candidate_floor == locked_floor
            } else {
                allowed_floors.contains(&candidate_floor)
            }
        })
        .collect();

    try_pick_weighted_candidate_with_retry(
        remaining_candidates,
        w1,
        w2,
        rng,
        |candidate| {
            let decision = candidate_advances_local_step(state, candidate.element_id, grid, &support_ids, has_stable);
            if !decision.accepted() {
                trace_event(
                    trace_logger,
                    SimulationTraceLevel::Info,
                    "sim.wf.retry_reject",
                    Some(scene_id),
                    Some(cycle_index),
                    Some(round_index),
                    Some(wf.id),
                    "candidate rejected during retry loop",
                    vec![
                        ("element_id".to_string(), candidate.element_id.to_string()),
                        ("reason".to_string(), decision.as_str().to_string()),
                        (
                            "buffer".to_string(),
                            format_ids(state.buffer_element_ids().iter().copied()),
                        ),
                    ],
                );
            }
            decision
        },
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

    true
}

fn current_workfront_anchor(wf: &SimWorkfront) -> (usize, usize) {
    (wf.grid_x, wf.grid_y)
}

fn collect_single_candidates(
    wf: &SimWorkfront,
    anchor: (usize, usize),
    grid: &SimGrid,
    support_ids: &HashSet<i32>,
    local_element_ids: &HashSet<i32>,
    committed_ids: &HashSet<i32>,
    allowed_floors: &HashSet<i32>,
    relax_locality: bool,
    node_pos: &HashMap<i32, (usize, usize, usize)>,
) -> Vec<SingleCandidate> {
    collect_single_candidates_optimized(
        wf,
        anchor,
        grid,
        support_ids,
        local_element_ids,
        committed_ids,
        allowed_floors,
        relax_locality,
        node_pos,
    )
}

fn collect_single_candidates_optimized(
    _wf: &SimWorkfront,
    anchor: (usize, usize),
    grid: &SimGrid,
    support_ids: &HashSet<i32>,
    local_element_ids: &HashSet<i32>,
    committed_ids: &HashSet<i32>,
    allowed_floors: &HashSet<i32>,
    relax_locality: bool,
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
        if !allowed_floors.contains(&floor) {
            continue;
        }
        let local_positions = local_positions_by_floor
            .get(&floor)
            .unwrap_or(&empty_positions);

        let Some(dist) = candidate_respects_locality(
            elem,
            floor,
            anchor,
            local_positions,
            relax_locality,
            node_pos,
        ) else {
            continue;
        };

        if !candidate_is_structurally_possible(elem, &support_nodes, node_pos) {
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

fn cached_element_floor(element_id: i32, grid: &SimGrid) -> Option<i32> {
    grid.element_floor_by_id.get(&element_id).copied()
}

fn resolve_element_floor(element_id: i32, grid: &SimGrid, dz: f64) -> Option<i32> {
    cached_element_floor(element_id, grid).or_else(|| element_floor(element_id, grid, dz))
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

fn trace_event(
    trace_logger: &mut Option<SimulationTraceLogger>,
    level: SimulationTraceLevel,
    event_name: &str,
    scene: Option<usize>,
    cycle: Option<usize>,
    round: Option<usize>,
    wf: Option<i32>,
    message: &str,
    fields: Vec<(String, String)>,
) {
    let Some(logger) = trace_logger.as_mut() else {
        return;
    };

    if !logger.level().allows(level) {
        return;
    }

    if logger.verbosity() == SimulationTraceVerbosity::Normal
        && matches!(event_name, "sim.round.start" | "sim.wf.pick")
    {
        return;
    }

    logger.emit(SimulationTraceEvent::new(
        level,
        event_name,
        scene,
        cycle,
        round,
        wf,
        message,
        fields,
    ));
}

fn format_local_step_compact(step: &LocalStep) -> String {
    format!(
        "floor={} pattern={} elements={}",
        step.floor,
        step.pattern,
        format_ids(step.element_ids.iter().copied())
    )
}

fn format_workfront_approved_steps(approved_steps: &[LocalStep]) -> String {
    approved_steps
        .iter()
        .enumerate()
        .map(|(index, step)| format!("#{} {}", index + 1, format_local_step_compact(step)))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn run_scenario_internal(
    scenario_id: usize,
    grid: &SimGrid,
    workfronts: &[SimWorkfront],
    seed: u64,
    weights: (f64, f64, f64),
    constraints: SimConstraints,
    cancel_flag: Option<&AtomicBool>,
    mut trace_logger: Option<SimulationTraceLogger>,
) -> SimScenario {
    let (w1, w2, w3) = weights;
    let mut rng = seed;

    let mut stable_ids: HashSet<i32> = HashSet::new();
    let mut steps: Vec<SimStep> = Vec::new();
    let mut approved_local_steps_by_wf: HashMap<i32, Vec<LocalStep>> = workfronts
        .iter()
        .map(|wf| (wf.id, Vec::new()))
        .collect();
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
    let mut cycle_index: usize = 0;

    trace_event(
        &mut trace_logger,
        SimulationTraceLevel::Info,
        "sim.run.start",
        Some(scenario_id),
        None,
        None,
        None,
        "simulation started",
        vec![
            ("seed".to_string(), seed.to_string()),
            (
                "grid".to_string(),
                format!("{}x{}x{}", grid.nx, grid.ny, grid.nz),
            ),
            ("workfronts".to_string(), workfronts.len().to_string()),
            (
                "upper_floor_threshold".to_string(),
                format!("{:.2}", constraints.upper_floor_column_rate_threshold),
            ),
            (
                "lower_completion_ratio".to_string(),
                format!("{:.2}", constraints.lower_floor_completion_ratio_threshold),
            ),
        ],
    );

    let termination_reason = 'outer: loop {
        cycle_index += 1;
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
        let mut cycle_no_local_step_rounds = 0u32;
        let mut cycle_rollback_count = 0u32;
        let mut cycle_round_index = 0usize;

        trace_event(
            &mut trace_logger,
            SimulationTraceLevel::Info,
            "sim.cycle.start",
            Some(scenario_id),
            Some(cycle_index),
            None,
            None,
            "cycle started",
            vec![
                ("stable_ids".to_string(), stable_ids.len().to_string()),
                ("committed_ids".to_string(), committed_ids.len().to_string()),
                (
                    "completed_wfs".to_string(),
                    format_ids(cycle_completed_wf.iter().copied()),
                ),
            ],
        );

        loop {
            cycle_round_index += 1;
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
            let committed_ids = compute_cycle_committed_ids(
                &stable_ids,
                &workfront_states,
                &cycle_local_steps,
            );
            let committed_floor_counts = floor_tracker.installed_per_floor_from(&committed_ids);

            // WF proportional control: scale active count by remaining work.
            // Each workfront can produce up to ~5 members per local step, so we treat
            // (workfronts × 5) as the capacity baseline for proportional scaling.
            let current_remaining = total_elements.saturating_sub(committed_ids.len());
            let wf_count = workfronts.len();
            let capacity_baseline = wf_count * 5;
            let active_count = if current_remaining == 0 {
                1
            } else if current_remaining >= capacity_baseline {
                wf_count
            } else {
                // Round up so we don't retire workfronts too aggressively
                std::cmp::max(1, (current_remaining * wf_count + capacity_baseline - 1) / capacity_baseline)
            };
            let active_wfs: Vec<&SimWorkfront> = if eligible_wfs.is_empty() {
                Vec::new()
            } else {
                let start_index = (cycle_round_index - 1) % eligible_wfs.len();
                eligible_wfs
                    .iter()
                    .cycle()
                    .skip(start_index)
                    .take(active_count.min(eligible_wfs.len()))
                    .copied()
                    .collect()
            };

            trace_event(
                &mut trace_logger,
                SimulationTraceLevel::Info,
                "sim.round.start",
                Some(scenario_id),
                Some(cycle_index),
                Some(cycle_round_index),
                None,
                "round started",
                vec![
                    (
                        "eligible_wfs".to_string(),
                        format_ids(eligible_wfs.iter().map(|wf| wf.id)),
                    ),
                    (
                        "active_wfs".to_string(),
                        format_ids(active_wfs.iter().map(|wf| wf.id)),
                    ),
                    (
                        "cycle_completed_wfs".to_string(),
                        format_ids(cycle_completed_wf.iter().copied()),
                    ),
                    (
                        "remaining".to_string(),
                        current_remaining.to_string(),
                    ),
                ],
            );

            total_sequence_rounds += 1;

            let mut selected_this_sequence: HashSet<i32> = HashSet::new();
            let mut sequence_installations: Vec<(i32, i32)> = Vec::new(); // (wf_id, element_id)

            // ── Pure Monte Carlo: each active WF picks ONE element randomly ──
            for wf in &active_wfs {
                let Some(state) = workfront_states.get_mut(&wf.id) else {
                    continue;
                };

                // Check if buffer became Invalid → rollback before picking
                let buf_ids = state.buffer_element_ids();
                let need_rollback = !buf_ids.is_empty()
                    && matches!(
                        classify_buffer(&buf_ids, grid, !stable_ids.is_empty()),
                        StepBufferDecision::Invalid
                    );

                if need_rollback {
                    let rollback_ids = state.buffer_element_ids();
                    for eid in &rollback_ids {
                        state.owned_ids.remove(eid);
                    }
                    state.buffer_sequences.clear();
                    state.committed_floor = None;
                    cycle_rollback_count += 1;
                    trace_event(
                        &mut trace_logger,
                        SimulationTraceLevel::Warning,
                        "sim.wf.rollback",
                        Some(scenario_id),
                        Some(cycle_index),
                        Some(cycle_round_index),
                        Some(wf.id),
                        "invalid buffer rollback",
                        vec![
                            ("rollback_ids".to_string(), format_ids(rollback_ids.iter().copied())),
                            ("reason".to_string(), "invalid_pattern".to_string()),
                        ],
                    );
                }

                let chosen_eid: Option<i32> = if stable_ids.is_empty() && cycle_local_steps.is_empty() {
                    // ── Bootstrap path ──
                    let bootstrap_candidates: Vec<Candidate> = generate_bootstrap_candidates(wf, grid, &node_pos)
                        .into_iter()
                        .filter(|candidate| {
                            candidate.element_ids.iter().all(|eid| {
                                !committed_ids.contains(eid) && !selected_this_sequence.contains(eid)
                            })
                        })
                        .collect();

                    if bootstrap_candidates.is_empty() {
                        None
                    } else {
                        let anchor_col = grid.column_starting_at(wf.grid_x, wf.grid_y, 0);
                        let preferred: Vec<&Candidate> = if let Some(anchor_id) = anchor_col {
                            let hits: Vec<&Candidate> = bootstrap_candidates
                                .iter()
                                .filter(|c| c.element_ids.contains(&anchor_id))
                                .collect();
                            if hits.is_empty() { bootstrap_candidates.iter().collect() } else { hits }
                        } else {
                            bootstrap_candidates.iter().collect()
                        };

                        let scores: Vec<f64> = preferred.iter().map(|c| c.score(w1, w2, w3)).collect();
                        let chosen_idx = weighted_random_choice(&scores, &mut rng);
                        let pattern = reorder_bootstrap_pattern(
                            &preferred[chosen_idx].element_ids, grid, &node_pos, wf,
                        );

                        // Bootstrap returns a full pattern — install all at once into buffer
                        let mut first_eid = None;
                        for &eid in &pattern {
                            if selected_this_sequence.contains(&eid) { continue; }
                            state.owned_ids.insert(eid);
                            state.buffer_sequences.push(SimSequence {
                                element_id: eid,
                                sequence_number: 0,
                            });
                            if state.committed_floor.is_none() {
                                state.committed_floor = resolve_element_floor(eid, grid, dz);
                            }
                            selected_this_sequence.insert(eid);
                            if first_eid.is_none() { first_eid = Some(eid); }
                        }
                        // Record first element for installation tracking
                        if let Some(eid) = first_eid {
                            sequence_installations.push((wf.id, eid));
                        }
                        trace_event(
                            &mut trace_logger,
                            SimulationTraceLevel::Info,
                            "sim.wf.pick",
                            Some(scenario_id),
                            Some(cycle_index),
                            Some(cycle_round_index),
                            Some(wf.id),
                            "bootstrap pattern installed",
                            vec![
                                ("pattern".to_string(), format_ids(pattern.iter().copied())),
                            ],
                        );
                        continue; // Bootstrap handled — skip to next WF
                    }
                } else {
                    // ── Incremental path: retry random candidates until one advances the local step ──
                    let mut wf_committed_ids = committed_ids.clone();
                    wf_committed_ids.extend(selected_this_sequence.iter().copied());

                    let anchor = current_workfront_anchor(wf);
                    let committed_floor = state.committed_floor;
                    let allowed_floors: HashSet<i32> = if let Some(locked_floor) = committed_floor {
                        std::iter::once(locked_floor).collect()
                    } else {
                        floor_tracker
                            .total_per_floor
                            .keys()
                            .copied()
                            .filter(|floor| {
                                is_floor_eligible_for_new_work(
                                    *floor,
                                    &committed_floor_counts,
                                    &floor_tracker.total_per_floor,
                                    &constraints,
                                )
                            })
                            .collect()
                    };
                    let picked = try_pick_incremental_candidate(
                        state,
                        wf,
                        anchor,
                        grid,
                        &stable_ids,
                        &cycle_local_steps,
                        &selected_this_sequence,
                        &wf_committed_ids,
                        &allowed_floors,
                        &floor_tracker,
                        &committed_floor_counts,
                        &constraints,
                        &node_pos,
                        w1,
                        w2,
                        &mut rng,
                        &mut trace_logger,
                        scenario_id,
                        cycle_index,
                        cycle_round_index,
                    );

                    if picked.is_none() {
                        // No candidates — rollback if locked
                        if committed_floor.is_some() {
                            let rollback_ids = state.buffer_element_ids();
                            for eid in &rollback_ids {
                                state.owned_ids.remove(eid);
                            }
                            state.buffer_sequences.clear();
                            state.committed_floor = None;
                            cycle_rollback_count += 1;
                            trace_event(
                                &mut trace_logger, SimulationTraceLevel::Warning,
                                "sim.wf.rollback", Some(scenario_id), Some(cycle_index),
                                Some(cycle_round_index), Some(wf.id),
                                "no candidates rollback",
                                vec![("rollback_ids".to_string(), format_ids(rollback_ids.iter().copied()))],
                            );
                        }
                        None
                    } else {
                        picked
                    }
                };

                // Install chosen element into buffer
                if let Some(eid) = chosen_eid {
                    let buffer_before = state.buffer_element_ids();
                    selected_this_sequence.insert(eid);
                    sequence_installations.push((wf.id, eid));

                    trace_event(
                        &mut trace_logger,
                        SimulationTraceLevel::Info,
                        "sim.wf.pick",
                        Some(scenario_id),
                        Some(cycle_index),
                        Some(cycle_round_index),
                        Some(wf.id),
                        "selected next element",
                        vec![
                            ("element_id".to_string(), eid.to_string()),
                            (
                                "member_type".to_string(),
                                get_element(grid, eid)
                                    .map(|e| e.member_type.clone())
                                    .unwrap_or_else(|| "Unknown".to_string()),
                            ),
                            (
                                "element_floor".to_string(),
                                resolve_element_floor(eid, grid, dz)
                                    .map(|f| f.to_string())
                                    .unwrap_or_else(|| "-".to_string()),
                            ),
                            ("buffer_before".to_string(), format_ids(buffer_before)),
                        ],
                    );
                }
            }

            // If no workfront could select anything, this sequence round is empty
            if sequence_installations.is_empty() {
                cycle_no_progress_count += 1;
                cycle_no_local_step_rounds += 1;
                if cycle_no_progress_count >= 10 {
                    break; // Stagnation within cycle
                }
                if cycle_no_local_step_rounds >= workfronts.len() as u32 * 10 {
                    break; // Deadlock: no local step produced for too long
                }
                continue;
            }
            cycle_no_progress_count = 0;

            // Add selected elements to workfront buffers (skip already-installed bootstrap elements)
            for &(wf_id, element_id) in &sequence_installations {
                if let Some(state) = workfront_states.get_mut(&wf_id) {
                    if state.owned_ids.contains(&element_id) {
                        continue; // bootstrap already installed
                    }
                    let was_empty = state.buffer_sequences.is_empty();
                    state.owned_ids.insert(element_id);
                    state.buffer_sequences.push(SimSequence {
                        element_id,
                        sequence_number: 0, // placeholder — will be reassigned by from_local_steps
                    });
                    if was_empty {
                        state.committed_floor = resolve_element_floor(element_id, grid, dz);
                    }
                }
            }

            // Check each eligible workfront for pattern completion
            // Use stable_ids + already-completed cycle local steps as context
            let cycle_stable_context = build_cycle_stable_context(&stable_ids, &cycle_local_steps);
            let local_steps_before_round = cycle_local_steps.len();

            for wf in &active_wfs {
                if cycle_completed_wf.contains(&wf.id) {
                    continue;
                }
                let Some(state) = workfront_states.get_mut(&wf.id) else {
                    continue;
                };

                let buffer_element_ids = state.buffer_element_ids();
                let decision = classify_buffer(&buffer_element_ids, grid, !stable_ids.is_empty());

                trace_event(
                    &mut trace_logger,
                    SimulationTraceLevel::Info,
                    "sim.wf.buffer_classified",
                    Some(scenario_id),
                    Some(cycle_index),
                    Some(cycle_round_index),
                    Some(wf.id),
                    "buffer classified",
                    vec![
                        (
                            "buffer".to_string(),
                            format_ids(buffer_element_ids.iter().copied()),
                        ),
                        (
                            "signature".to_string(),
                            buffer_element_ids
                                .iter()
                                .map(|eid| if is_column(grid, *eid) { 'C' } else { 'G' })
                                .collect::<String>(),
                        ),
                        ("decision".to_string(), format!("{:?}", decision)),
                        (
                            "has_stable_structure".to_string(),
                            (!stable_ids.is_empty()).to_string(),
                        ),
                        (
                            "committed_floor".to_string(),
                            state.committed_floor
                                .map(|floor| floor.to_string())
                                .unwrap_or_else(|| "-".to_string()),
                        ),
                    ],
                );

                if let StepBufferDecision::Complete(pattern) = decision {
                    let emit_result = try_emit_completed_buffer(
                        wf.id,
                        state,
                        &pattern,
                        grid,
                        dz,
                        &cycle_stable_context,
                        &mut cycle_local_steps,
                        &mut cycle_completed_wf,
                        &mut trace_logger,
                        scenario_id,
                        cycle_index,
                        cycle_round_index,
                    );

                    if emit_result == EmitResult::Infeasible {
                        // Buffer failed stability — rollback buffer elements
                        let rollback_ids = state.buffer_element_ids();
                        for eid in &rollback_ids {
                            state.owned_ids.remove(eid);
                        }
                        state.buffer_sequences.clear();
                        state.committed_floor = None;

                        cycle_rollback_count += 1;
                        trace_event(
                            &mut trace_logger,
                            SimulationTraceLevel::Warning,
                            "sim.wf.infeasible_rollback",
                            Some(scenario_id),
                            Some(cycle_index),
                            Some(cycle_round_index),
                            Some(wf.id),
                            "infeasible buffer rolled back",
                            vec![
                                ("rollback_ids".to_string(), format_ids(rollback_ids.iter().copied())),
                                ("pattern".to_string(), pattern.as_str().to_string()),
                            ],
                        );
                    } else if emit_result == EmitResult::Emitted {
                        if let Some(approved_step) = cycle_local_steps.last().cloned() {
                            let approved_steps = approved_local_steps_by_wf
                                .entry(wf.id)
                                .or_default();
                            approved_steps.push(approved_step.clone());

                            trace_event(
                                &mut trace_logger,
                                SimulationTraceLevel::Info,
                                "sim.wf.approved_local_step_recorded",
                                Some(scenario_id),
                                Some(cycle_index),
                                Some(cycle_round_index),
                                Some(wf.id),
                                "approved local step recorded for workfront",
                                vec![
                                    (
                                        "approved_count".to_string(),
                                        approved_steps.len().to_string(),
                                    ),
                                    (
                                        "latest_local_step".to_string(),
                                        format_local_step_compact(&approved_step),
                                    ),
                                    (
                                        "approved_local_steps".to_string(),
                                        format_workfront_approved_steps(approved_steps),
                                    ),
                                ],
                            );
                        }
                    }
                }
            }

            let local_step_added = cycle_local_steps.len() > local_steps_before_round;

            if !local_step_added {
                cycle_no_local_step_rounds += 1;
            } else {
                cycle_no_local_step_rounds = 0;
            }

            // If all workfronts have completed a local step in this cycle, end cycle
            if cycle_completed_wf.len() >= workfronts.len() {
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
            trace_event(
                &mut trace_logger,
                SimulationTraceLevel::Warning,
                "sim.cycle.end",
                Some(scenario_id),
                Some(cycle_index),
                None,
                None,
                "cycle ended without local steps",
                vec![
                    ("local_steps".to_string(), "0".to_string()),
                    (
                        "empty_cycles".to_string(),
                        consecutive_empty_cycles.to_string(),
                    ),
                ],
            );
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
        trace_event(
            &mut trace_logger,
            SimulationTraceLevel::Info,
            "sim.cycle.end",
            Some(scenario_id),
            Some(cycle_index),
            None,
            None,
            "cycle ended",
            vec![
                (
                    "total_elements".to_string(),
                    total_elements.to_string(),
                ),
                (
                    "remaining".to_string(),
                    total_elements.saturating_sub(stable_ids.len()).to_string(),
                ),
                (
                    "active_wf_count".to_string(),
                    workfronts.len().to_string(),
                ),
                (
                    "local_steps".to_string(),
                    step.local_steps.len().to_string(),
                ),
                (
                    "step_members".to_string(),
                    step.element_ids.len().to_string(),
                ),
                ("step_rounds".to_string(), round_count.to_string()),
                (
                    "stable_ids_after".to_string(),
                    stable_ids.len().to_string(),
                ),
                (
                    "empty_rounds_in_cycle".to_string(),
                    cycle_no_local_step_rounds.to_string(),
                ),
                (
                    "rollback_count".to_string(),
                    cycle_rollback_count.to_string(),
                ),
            ],
        );
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

    trace_event(
        &mut trace_logger,
        SimulationTraceLevel::Info,
        "sim.run.end",
        Some(scenario_id),
        None,
        None,
        None,
        "simulation finished",
        vec![
            ("termination".to_string(), termination_reason.to_string()),
            ("total_steps".to_string(), total_steps.to_string()),
            (
                "total_sequence_rounds".to_string(),
                total_sequence_rounds.to_string(),
            ),
        ],
    );

    if let Some(logger) = trace_logger.as_mut() {
        logger.flush();
    }

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
            throttle_events: 0,
            floor_rebase_events: 0,
            spatial_rebase_events: 0,
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
        lower_floor_completion_ratio_threshold: 0.5,
    };
    run_scenario_internal(
        scenario_id,
        grid,
        workfronts,
        seed,
        weights,
        constraints,
        None,
        None,
    )
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
        lower_floor_completion_ratio_threshold: 0.5,
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
                        throttle_events: 0,
                        floor_rebase_events: 0,
                        spatial_rebase_events: 0,
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
                None,
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

pub fn run_all_scenarios_with_progress_and_cancel_and_trace(
    count: usize,
    grid: &SimGrid,
    workfronts: &[SimWorkfront],
    weights: (f64, f64, f64),
    constraints: SimConstraints,
    progress_counter: Option<Arc<AtomicUsize>>,
    cancel_flag: Option<Arc<AtomicBool>>,
    trace_config: SimulationTraceConfig,
    trace_run_context: Option<SimulationTraceRunContext>,
) -> (Vec<SimScenario>, Vec<std::path::PathBuf>, String) {
    let mut results: Vec<(SimScenario, Option<std::path::PathBuf>, Option<String>)> =
        (1..=count)
            .into_par_iter()
            .map(|i| {
                if cancel_flag
                    .as_ref()
                    .map(|flag| flag.load(Ordering::Relaxed))
                    .unwrap_or(false)
                {
                    return (
                        SimScenario {
                            id: i,
                            seed: i as u64 * 2654435761,
                            steps: Vec::new(),
                            metrics: ScenarioMetrics {
                                avg_members_per_step: 0.0,
                                avg_connectivity: 0.0,
                                total_steps: 0,
                                total_members_installed: 0,
                                termination_reason: TerminationReason::Cancelled,
                                throttle_events: 0,
                                floor_rebase_events: 0,
                                spatial_rebase_events: 0,
                            },
                        },
                        None,
                        None,
                    );
                }

                let seed = i as u64 * 2654435761;
                let mut trace_path = None;
                let mut trace_error = None;
                let trace_logger = if trace_config.enabled {
                    if let Some(run_context) = trace_run_context.as_ref() {
                        match SimulationTraceLogger::create_for_scene(
                            trace_config.clone(),
                            run_context,
                            i,
                        ) {
                            Ok(logger) => {
                                trace_path = Some(logger.output_path());
                                Some(logger)
                            }
                            Err(err) => {
                                trace_error = Some(format!(
                                    "scene {} trace init failed: {}",
                                    i, err
                                ));
                                None
                            }
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };

                let scenario = run_scenario_internal(
                    i,
                    grid,
                    workfronts,
                    seed,
                    weights,
                    constraints,
                    cancel_flag.as_deref(),
                    trace_logger,
                );
                if let Some(counter) = &progress_counter {
                    counter.fetch_add(1, Ordering::Relaxed);
                }

                (scenario, trace_path, trace_error)
            })
            .collect();

    results.sort_by_key(|entry| entry.0.id);

    let mut scenarios: Vec<SimScenario> = Vec::with_capacity(results.len());
    let mut trace_paths: Vec<std::path::PathBuf> = Vec::new();
    let mut trace_errors: Vec<String> = Vec::new();

    for (scenario, trace_path, trace_error) in results {
        scenarios.push(scenario);
        if let Some(path) = trace_path {
            trace_paths.push(path);
        }
        if let Some(err) = trace_error {
            trace_errors.push(err);
        }
    }

    let trace_status = if trace_config.enabled {
        if trace_errors.is_empty() {
            format!("Trace logs generated: {} file(s)", trace_paths.len())
        } else {
            format!(
                "Trace logs partially generated: {} file(s), {} error(s)",
                trace_paths.len(),
                trace_errors.len()
            )
        }
    } else {
        String::new()
    };

    (scenarios, trace_paths, trace_status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim_grid::SimGrid;

    fn local_step_is_connected(grid: &SimGrid, element_ids: &[i32]) -> bool {
        if element_ids.len() <= 1 {
            return true;
        }

        let mut node_to_elements: HashMap<i32, Vec<i32>> = HashMap::new();
        for element_id in element_ids {
            let Some(element) = get_element(grid, *element_id) else {
                return false;
            };
            node_to_elements
                .entry(element.node_i_id)
                .or_default()
                .push(*element_id);
            node_to_elements
                .entry(element.node_j_id)
                .or_default()
                .push(*element_id);
        }

        let mut visited: HashSet<i32> = HashSet::new();
        let mut queue: Vec<i32> = vec![element_ids[0]];

        while let Some(current_id) = queue.pop() {
            if !visited.insert(current_id) {
                continue;
            }

            let Some(element) = get_element(grid, current_id) else {
                return false;
            };
            for node_id in [element.node_i_id, element.node_j_id] {
                if let Some(neighbors) = node_to_elements.get(&node_id) {
                    for neighbor_id in neighbors {
                        if !visited.contains(neighbor_id) {
                            queue.push(*neighbor_id);
                        }
                    }
                }
            }
        }

        visited.len() == element_ids.len()
    }

    fn make_grid_4x8x3() -> SimGrid {
        SimGrid::new(4, 8, 3, 6000.0, 6000.0, 4000.0)
    }

    fn make_workfronts_4x8_congestion_case() -> Vec<SimWorkfront> {
        vec![
            SimWorkfront { id: 1, grid_x: 1, grid_y: 0 },
            SimWorkfront { id: 2, grid_x: 2, grid_y: 0 },
            SimWorkfront { id: 3, grid_x: 3, grid_y: 0 },
            SimWorkfront { id: 4, grid_x: 3, grid_y: 2 },
            SimWorkfront { id: 5, grid_x: 3, grid_y: 4 },
            SimWorkfront { id: 6, grid_x: 3, grid_y: 6 },
        ]
    }

    #[test]
    fn test_4x8x3_congestion_fixture_matches_seed_columns() {
        let grid = make_grid_4x8x3();

        let seed_columns = [
            grid.column_starting_at(1, 0, 0),
            grid.column_starting_at(2, 0, 0),
            grid.column_starting_at(3, 0, 0),
            grid.column_starting_at(3, 2, 0),
            grid.column_starting_at(3, 4, 0),
            grid.column_starting_at(3, 6, 0),
        ];

        assert_eq!(seed_columns, [Some(9), Some(17), Some(25), Some(27), Some(29), Some(31)]);
    }

    #[test]
    fn test_simulation_4x8x3_congestion_seed_batch_does_not_worsen() {
        let grid = make_grid_4x8x3();
        let wfs = make_workfronts_4x8_congestion_case();
        let floor_tracker = FloorTracker::from_grid(&grid, grid_dz(&grid));
        let seeds = [
            2654435761_u64,
            5308871522,
            7963307283,
            10617743044,
            13272178805,
            15926614566,
            18581050327,
            21235486088,
            23889921849,
            26544357610,
        ];

        let mut failures: Vec<(usize, TerminationReason)> = Vec::new();
        let mut first_failed_scenario: Option<SimScenario> = None;
        for (index, seed) in seeds.iter().copied().enumerate() {
            let scenario = run_scenario_internal(
                index + 1,
                &grid,
                &wfs,
                seed,
                (0.5, 0.3, 0.2),
                SimConstraints {
                    upper_floor_column_rate_threshold: 0.3,
                    lower_floor_completion_ratio_threshold: 0.5,
                },
                None,
                None,
            );

            if !matches!(scenario.metrics.termination_reason, TerminationReason::Completed) {
                if first_failed_scenario.is_none() {
                    first_failed_scenario = Some(scenario.clone());
                }
                failures.push((index + 1, scenario.metrics.termination_reason.clone()));
            }
        }

        println!(
            "4x8x3 default batch summary: completed={}, failed={:?}",
            seeds.len() - failures.len(),
            failures
        );

        if let Some(scenario) = first_failed_scenario {
            let mut installed_ids: HashSet<i32> = HashSet::new();
            for (step_idx, step) in scenario.steps.iter().enumerate() {
                installed_ids.extend(step.element_ids.iter().copied());
                let floor_counts = floor_tracker.installed_per_floor_from(&installed_ids);
                let step_floors: Vec<i32> = step
                    .element_ids
                    .iter()
                    .filter_map(|eid| grid.element_floor_by_id.get(eid).copied())
                    .collect();
                println!(
                    "failed scenario step {}: members={}, floors={:?}, cumulative_cols={:?}",
                    step_idx + 1,
                    step.element_ids.len(),
                    step_floors,
                    floor_counts
                );
            }

            let all_ids: HashSet<i32> = grid.elements.iter().map(|e| e.id).collect();
            let remaining: HashSet<i32> = all_ids.difference(&installed_ids).copied().collect();
            let mut remaining_by_floor: HashMap<i32, (usize, usize)> = HashMap::new();
            for eid in &remaining {
                let floor = grid.element_floor_by_id.get(eid).copied().unwrap_or(0);
                let is_column = is_column(&grid, *eid);
                let entry = remaining_by_floor.entry(floor).or_insert((0, 0));
                if is_column {
                    entry.0 += 1;
                } else {
                    entry.1 += 1;
                }
            }
            println!(
                "failed scenario summary: steps={}, members_installed={}, throttle_events={}, remaining_by_floor={:?}",
                scenario.metrics.total_steps,
                scenario.metrics.total_members_installed,
                scenario.metrics.throttle_events,
                remaining_by_floor
            );
        }

        assert!(
            failures.is_empty(),
            "4x8x3 congestion batch should have 0 failures, got {:?}",
            failures
        );
    }

    #[test]
    fn test_buffer_locality_excludes_distant_ground_column() {
        let grid = make_grid_4x8x3();
        let node_pos = build_node_pos(&grid);
        let wf = SimWorkfront { id: 6, grid_x: 3, grid_y: 6 };
        let anchor = current_workfront_anchor(&wf);
        let support_ids: HashSet<i32> = HashSet::from([15, 16]);
        let buffer_local_ids: HashSet<i32> = HashSet::from([16]);
        let committed_ids: HashSet<i32> = HashSet::new();
        let allowed_floors: HashSet<i32> = HashSet::from([1]);

        let candidates = collect_single_candidates(
            &wf,
            anchor,
            &grid,
            &support_ids,
            &buffer_local_ids,
            &committed_ids,
            &allowed_floors,
            false,
            &node_pos,
        );

        assert!(
            !candidates.iter().any(|candidate| candidate.element_id == 30),
            "buffer-seeded locality must exclude distant ground column 30 from the in-progress local step candidates"
        );
    }

    #[test]
    fn test_locality_seed_ids_prefers_buffer_over_owned_history() {
        let mut state = WorkfrontState::default();
        state.owned_ids = HashSet::from([30, 31]);
        state.buffer_sequences = vec![
            SimSequence {
                element_id: 16,
                sequence_number: 1,
            },
            SimSequence {
                element_id: 78,
                sequence_number: 2,
            },
        ];

        let locality_seed_ids = locality_seed_ids_for_search(&state);

        assert_eq!(locality_seed_ids, HashSet::from([16, 78]));
        assert!(!locality_seed_ids.contains(&30));
    }

    #[test]
    fn test_weighted_retry_rejects_bad_pick_and_finds_next_candidate() {
        let retry_candidates = vec![
            SingleCandidate {
                element_id: 30,
                connectivity: 1,
                frontier_dist: 0.0,
            },
            SingleCandidate {
                element_id: 16,
                connectivity: 0,
                frontier_dist: 0.0,
            },
        ];
        let mut rng = 0_u64;
        let mut attempted_ids: Vec<i32> = Vec::new();

        let picked = try_pick_weighted_candidate_with_retry(
            retry_candidates.iter().collect(),
            1.0,
            0.0,
            &mut rng,
            |candidate| {
                attempted_ids.push(candidate.element_id);
                if candidate.element_id == 30 {
                    CandidateAdvanceResult::RejectedInvalidPattern
                } else {
                    CandidateAdvanceResult::Accepted
                }
            },
        );

        assert_eq!(picked, Some(16));
        assert_eq!(attempted_ids, vec![30, 16]);
    }

    #[test]
    fn test_seed_1_emitted_local_steps_are_connected_assemblies() {
        let grid = make_grid_4x8x3();
        let wfs = make_workfronts_4x8_congestion_case();
        let scenario = run_scenario_internal(
            1,
            &grid,
            &wfs,
            2654435761_u64,
            (0.5, 0.3, 0.2),
            SimConstraints {
                upper_floor_column_rate_threshold: 0.3,
                lower_floor_completion_ratio_threshold: 0.5,
            },
            None,
            None,
        );

        for (step_index, step) in scenario.steps.iter().enumerate() {
            for local_step in &step.local_steps {
                assert!(
                    local_step_is_connected(&grid, &local_step.element_ids),
                    "Step {} contains a disconnected local assembly: wf={} pattern={} elements={:?}",
                    step_index + 1,
                    local_step.workfront_id,
                    local_step.pattern,
                    local_step.element_ids,
                );
            }
        }
    }
}
