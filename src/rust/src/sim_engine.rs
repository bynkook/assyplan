//! Simulation Engine for AssyPlan Phase 3
//!
//! Sequence and step are separate concepts:
//! - sequence: individual member installation order
//! - step: pattern-based stability unit that may contain multiple sequences

use std::collections::{HashMap, HashSet};

use rayon::prelude::*;

use crate::graphics::ui::{
    ScenarioMetrics, SimScenario, SimSequence, SimStep, SimWorkfront, TerminationReason,
};
use crate::sim_grid::SimGrid;
use crate::stability::{
    has_minimum_assembly, validate_column_support, validate_girder_support, StabilityElement,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PatternType {
    Col,
    Girder,
    ColCol,
    ColGirder,
    GirderGirder,
    ColColGirder,
    ColGirderCol,
    ColGirderGirder,
    ColColGirderGirder,
    ColColGirderColGirder,
    Bootstrap,
}

impl PatternType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Col => "Col",
            Self::Girder => "Girder",
            Self::ColCol => "ColCol",
            Self::ColGirder => "ColGirder",
            Self::GirderGirder => "GirderGirder",
            Self::ColColGirder => "ColColGirder",
            Self::ColGirderCol => "ColGirderCol",
            Self::ColGirderGirder => "ColGirderGirder",
            Self::ColColGirderGirder => "ColColGirderGirder",
            Self::ColColGirderColGirder => "ColColGirderColGirder",
            Self::Bootstrap => "Bootstrap",
        }
    }
}

#[derive(Clone, Debug)]
struct PatternChoice {
    element_ids: Vec<i32>,
    pattern: PatternType,
}

/// A single-element candidate for seed selection.
#[derive(Clone, Debug)]
pub struct SingleCandidate {
    pub element_id: i32,
    pub connectivity: usize,
    pub frontier_dist: f64,
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

/// Bootstrap candidate only (first step: 3 columns + 2 girders bundle).
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

fn grid_dz(grid: &SimGrid) -> f64 {
    let mut z_vals: Vec<i64> = grid
        .nodes
        .iter()
        .map(|n| (n.z * 1000.0).round() as i64)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    z_vals.sort();
    if z_vals.len() >= 2 {
        (z_vals[1] - z_vals[0]) as f64 / 1000.0
    } else {
        4000.0
    }
}

fn build_node_pos(grid: &SimGrid) -> HashMap<i32, (usize, usize, usize)> {
    grid.node_index
        .iter()
        .map(|(&pos, &id)| (id, pos))
        .collect()
}

fn get_element(grid: &SimGrid, element_id: i32) -> Option<&StabilityElement> {
    grid.elements.iter().find(|e| e.id == element_id)
}

fn is_column(grid: &SimGrid, element_id: i32) -> bool {
    get_element(grid, element_id)
        .map(|e| e.member_type == "Column")
        .unwrap_or(false)
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

fn check_single_stability(element_id: i32, grid: &SimGrid, installed_ids: &HashSet<i32>) -> bool {
    let Some(elem) = get_element(grid, element_id) else {
        return false;
    };
    if elem.member_type == "Column" {
        validate_column_support(elem, &grid.nodes, &grid.elements, installed_ids)
    } else {
        validate_girder_support(elem, &grid.nodes, &grid.elements, installed_ids)
    }
}

fn check_bundle_stability(
    element_ids: &[i32],
    grid: &SimGrid,
    installed_ids: &HashSet<i32>,
) -> bool {
    let mut combined: HashSet<i32> = installed_ids.clone();
    combined.extend(element_ids.iter().copied());

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
        let Some(elem) = get_element(grid, *eid) else {
            return false;
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

fn check_upper_floor_constraint(
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

        if installed_lower == 0 {
            continue;
        }
        if total_lower > 0 && installed_lower >= total_lower {
            continue;
        }

        let installed_upper = *installed_per_floor.get(&floor).unwrap_or(&0) + 1;
        let ratio = installed_upper as f64 / installed_lower as f64;
        if ratio > threshold {
            return false;
        }
    }

    true
}

fn min_unstarted_floor(
    _wf: &SimWorkfront,
    grid: &SimGrid,
    installed_ids: &HashSet<i32>,
    node_pos: &HashMap<i32, (usize, usize, usize)>,
) -> usize {
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

    let mut min_girder_floor_zi: Option<usize> = None;
    for elem in &grid.elements {
        if elem.member_type != "Girder" || installed_ids.contains(&elem.id) {
            continue;
        }
        if let Some(&(_, _, zi_g)) = node_pos.get(&elem.node_i_id) {
            let floor_zi = zi_g.saturating_sub(1);
            min_girder_floor_zi = Some(match min_girder_floor_zi {
                Option::None => floor_zi,
                Some(prev) => prev.min(floor_zi),
            });
        }
    }
    min_girder_floor_zi.unwrap_or(grid.nz.saturating_sub(1))
}

fn collect_single_candidates(
    wf: &SimWorkfront,
    grid: &SimGrid,
    installed_ids: &HashSet<i32>,
    installed_nodes: &HashSet<i32>,
    node_pos: &HashMap<i32, (usize, usize, usize)>,
) -> Vec<SingleCandidate> {
    let zi_target = min_unstarted_floor(wf, grid, installed_ids, node_pos);

    for attempt in 0..2usize {
        let search_zi = zi_target + attempt;
        if search_zi >= grid.nz.saturating_sub(1) {
            break;
        }

        let mut result: Vec<SingleCandidate> = Vec::new();
        let mut seen: HashSet<i32> = HashSet::new();
        let is_lowest = search_zi == 0;

        for &nid in installed_nodes {
            let Some(&(xi, yi, zi)) = node_pos.get(&nid) else {
                continue;
            };
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

        let upper_zi = search_zi + 1;
        let mut upper_nodes: HashSet<i32> = HashSet::new();
        for sc in &result {
            if let Some(e) = get_element(grid, sc.element_id) {
                upper_nodes.insert(e.node_j_id);
            }
        }
        for &eid in installed_ids {
            if let Some(e) = get_element(grid, eid) {
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
                    is_lowest_floor: false,
                });
            }
        }

        if !result.is_empty() {
            return result;
        }
    }

    let zi_target = min_unstarted_floor(wf, grid, installed_ids, node_pos);
    for zi in zi_target..=(grid.nz.saturating_sub(1)) {
        let mut result: Vec<SingleCandidate> = Vec::new();
        let mut seen: HashSet<i32> = HashSet::new();

        for elem in &grid.elements {
            if installed_ids.contains(&elem.id) {
                continue;
            }
            let elem_zi = node_pos.get(&elem.node_i_id).map(|p| p.2).unwrap_or(9999);
            let on_this_floor = if elem.member_type == "Column" {
                elem_zi == zi
            } else {
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

    Vec::new()
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
    pattern: PatternType,
    grid: &SimGrid,
    installed_ids: &HashSet<i32>,
    threshold: f64,
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
    if !check_upper_floor_constraint(&element_ids, grid, installed_ids, threshold) {
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

/// Returns the (dx_sign, dy_sign) direction of a girder element.
/// (±1, 0) means X-axis aligned, (0, ±1) means Y-axis aligned.
fn girder_axis(eid: i32, grid: &SimGrid) -> Option<(i8, i8)> {
    let e = get_element(grid, eid)?;
    let (xi, yi, _) = grid.node_coords(e.node_i_id)?;
    let (xj, yj, _) = grid.node_coords(e.node_j_id)?;
    let dx = xj - xi;
    let dy = yj - yi;
    Some((
        if dx.abs() > 0.001 {
            dx.signum() as i8
        } else {
            0
        },
        if dy.abs() > 0.001 {
            dy.signum() as i8
        } else {
            0
        },
    ))
}

fn contains_forbidden_pattern(element_ids: &[i32], grid: &SimGrid) -> bool {
    let types: Vec<&str> = element_ids
        .iter()
        .filter_map(|eid| get_element(grid, *eid).map(|e| e.member_type.as_str()))
        .collect();

    // Forbid 3+ consecutive columns (regardless of girders after)
    if matches!(
        types.as_slice(),
        ["Column", "Column", "Column"] | ["Column", "Column", "Column", "Girder"]
    ) {
        return true;
    }

    // Forbid 2+ girders in the same axis direction (parallel girders)
    // A valid multi-girder bundle must have girders in perpendicular directions.
    let girder_axes: Vec<(i8, i8)> = element_ids
        .iter()
        .filter(|&&eid| {
            get_element(grid, eid)
                .map(|e| e.member_type == "Girder")
                .unwrap_or(false)
        })
        .filter_map(|&eid| girder_axis(eid, grid))
        .collect();

    if girder_axes.len() >= 2 {
        // Check every pair — if any two are parallel (same axis), forbid the bundle
        for i in 0..girder_axes.len() {
            for j in (i + 1)..girder_axes.len() {
                let (ax, ay) = girder_axes[i];
                let (bx, by) = girder_axes[j];
                // Parallel if they share the same non-zero axis component
                // (1,0) ∥ (1,0), (-1,0) ∥ (1,0), (0,1) ∥ (0,-1) etc.
                let dot = ax as i32 * bx as i32 + ay as i32 * by as i32;
                // dot != 0 means not perpendicular → parallel or anti-parallel → forbidden
                if dot != 0 {
                    return true;
                }
            }
        }
    }

    false
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

fn bundle_score(
    element_ids: &[i32],
    grid: &SimGrid,
    installed_nodes: &HashSet<i32>,
    node_pos: &HashMap<i32, (usize, usize, usize)>,
    w1: f64,
    w2: f64,
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
    w1 * connectivity_sum as f64 + dist_score + size_bonus
}

fn try_build_pattern(
    seed_id: i32,
    grid: &SimGrid,
    installed_ids: &HashSet<i32>,
    installed_nodes: &HashSet<i32>,
    node_pos: &HashMap<i32, (usize, usize, usize)>,
    threshold: f64,
    w1: f64,
    w2: f64,
    rng: &mut u64,
) -> (Vec<i32>, String) {
    let Some(seed) = get_element(grid, seed_id) else {
        return (Vec::new(), PatternType::Col.as_str().to_string());
    };

    let mut choices: Vec<PatternChoice> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    if seed.member_type == "Girder" {
        push_pattern_if_valid(
            &mut choices,
            &mut seen,
            vec![seed_id],
            PatternType::Girder,
            grid,
            installed_ids,
            threshold,
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
                PatternType::GirderGirder,
                grid,
                installed_ids,
                threshold,
            );
        }
    } else {
        push_pattern_if_valid(
            &mut choices,
            &mut seen,
            vec![seed_id],
            PatternType::Col,
            grid,
            installed_ids,
            threshold,
        );

        let seed_upper = seed.node_j_id;

        for col2 in uninstalled_adjacent_columns(seed_id, grid, installed_ids, node_pos) {
            push_pattern_if_valid(
                &mut choices,
                &mut seen,
                vec![seed_id, col2],
                PatternType::ColCol,
                grid,
                installed_ids,
                threshold,
            );

            if let Some(col2_elem) = get_element(grid, col2) {
                if let Some(g1) = grid.girder_between(seed_upper, col2_elem.node_j_id) {
                    if !installed_ids.contains(&g1) {
                        push_pattern_if_valid(
                            &mut choices,
                            &mut seen,
                            vec![seed_id, col2, g1],
                            PatternType::ColColGirder,
                            grid,
                            installed_ids,
                            threshold,
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
                                PatternType::ColColGirderGirder,
                                grid,
                                installed_ids,
                                threshold,
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
                                            PatternType::ColColGirderColGirder,
                                            grid,
                                            installed_ids,
                                            threshold,
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
                PatternType::ColGirder,
                grid,
                installed_ids,
                threshold,
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
                            PatternType::ColGirderCol,
                            grid,
                            installed_ids,
                            threshold,
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
                            PatternType::ColGirderGirder,
                            grid,
                            installed_ids,
                            threshold,
                        );
                    }
                }
            }
        }
    }

    if choices.is_empty() {
        return if seed.member_type == "Girder" {
            (vec![seed_id], PatternType::Girder.as_str().to_string())
        } else {
            (vec![seed_id], PatternType::Col.as_str().to_string())
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
        .map(|choice| bundle_score(&choice.element_ids, grid, installed_nodes, node_pos, w1, w2))
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

fn last_sequence_number(steps: &[SimStep]) -> usize {
    steps
        .last()
        .and_then(|step| step.sequences.last())
        .map(|seq: &SimSequence| seq.sequence_number)
        .unwrap_or(0)
}

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

    let mut installed_ids: HashSet<i32> = HashSet::new();
    let mut installed_nodes: HashSet<i32> = HashSet::new();
    let mut steps: Vec<SimStep> = Vec::new();

    let total_elements = grid.elements.len();
    let node_pos = build_node_pos(grid);
    let dz = grid_dz(grid);

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

        // All workfronts contribute candidates every step (merged pool).
        // Pick the workfront whose chosen candidate wins the score competition.
        // Round-robin index is used only as tiebreak / wf_id assignment fallback.
        let wf_idx_fallback = ((global_step_count - 1) as usize) % workfronts.len();

        if installed_ids.is_empty() {
            // Bootstrap: try all workfronts, merge candidates, pick best
            let mut all_bootstrap: Vec<(usize, Candidate)> = Vec::new();
            for (wf_idx, wf) in workfronts.iter().enumerate() {
                for c in generate_bootstrap_candidates(wf, grid, &node_pos) {
                    all_bootstrap.push((wf_idx, c));
                }
            }
            let valid: Vec<(usize, &Candidate)> = all_bootstrap
                .iter()
                .filter(|(_, c)| check_bundle_stability(&c.element_ids, grid, &installed_ids))
                .map(|(wf_idx, c)| (*wf_idx, c))
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

            let scores: Vec<f64> = valid.iter().map(|(_, c)| c.score(w1, w2, w3)).collect();
            let best_idx = weighted_random_choice(&scores, &mut rng);
            let (chosen_wf_idx, chosen) = valid[best_idx];
            let wf = &workfronts[chosen_wf_idx];

            let step_floor = chosen
                .element_ids
                .iter()
                .find_map(|eid| element_floor(*eid, grid, dz))
                .unwrap_or(1);

            let step = SimStep::from_elements(
                wf.id,
                chosen.element_ids.clone(),
                step_floor,
                PatternType::Bootstrap.as_str(),
                last_sequence_number(&steps) + 1,
            );

            for eid in &step.element_ids {
                installed_ids.insert(*eid);
                if let Some(e) = get_element(grid, *eid) {
                    installed_nodes.insert(e.node_i_id);
                    installed_nodes.insert(e.node_j_id);
                }
            }

            members_added_last_300.push(step.element_ids.len());
            steps.push(step);
            continue;
        }

        // Merge candidates from ALL workfronts — each workfront contributes its
        // own frontier in parallel; the best seed across all of them is chosen.
        let mut all_candidates: Vec<(usize, SingleCandidate)> = Vec::new();
        for (wf_idx, wf) in workfronts.iter().enumerate() {
            for c in
                collect_single_candidates(wf, grid, &installed_ids, &installed_nodes, &node_pos)
            {
                all_candidates.push((wf_idx, c));
            }
        }
        // Deduplicate by element_id (same element reachable from multiple wf's)
        {
            let mut seen_eids: HashSet<i32> = HashSet::new();
            all_candidates.retain(|(_, c)| seen_eids.insert(c.element_id));
        }

        if all_candidates.is_empty() {
            consecutive_no_candidates += 1;
            members_added_last_300.push(0);
            if consecutive_no_candidates >= 10 {
                break TerminationReason::NoCandidates;
            }
            continue;
        }
        consecutive_no_candidates = 0;

        let after_stability: Vec<(usize, &SingleCandidate)> = all_candidates
            .iter()
            .filter(|(_, c)| check_single_stability(c.element_id, grid, &installed_ids))
            .map(|(wf_idx, c)| (*wf_idx, c))
            .collect();

        let valid: Vec<(usize, &&SingleCandidate)> = after_stability
            .iter()
            .filter(|(_, c)| {
                check_upper_floor_constraint(&[c.element_id], grid, &installed_ids, threshold)
            })
            .map(|(wf_idx, c)| (*wf_idx, c))
            .collect();

        if valid.is_empty() {
            if !after_stability.is_empty() {
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

        let scores: Vec<f64> = valid.iter().map(|(_, c)| c.score(w1, w2)).collect();
        let best_valid_idx = weighted_random_choice(&scores, &mut rng);
        let (chosen_wf_idx, chosen_cand) = valid[best_valid_idx];
        let chosen_seed = chosen_cand.element_id;
        // Use the workfront that owns the chosen seed for wf_id attribution
        let wf = &workfronts[chosen_wf_idx];
        let _ = wf_idx_fallback; // suppress unused warning

        let (element_ids, pattern) = try_build_pattern(
            chosen_seed,
            grid,
            &installed_ids,
            &installed_nodes,
            &node_pos,
            threshold,
            w1,
            w2,
            &mut rng,
        );

        let step_floor = element_ids
            .iter()
            .find(|eid| is_column(grid, **eid))
            .and_then(|eid| element_floor(*eid, grid, dz))
            .or_else(|| {
                element_ids
                    .first()
                    .and_then(|eid| element_floor(*eid, grid, dz))
            })
            .unwrap_or_else(|| steps.last().map(|s| s.floor).unwrap_or(1));

        let step = SimStep::from_elements(
            wf.id,
            element_ids,
            step_floor,
            pattern,
            last_sequence_number(&steps) + 1,
        );

        for eid in &step.element_ids {
            installed_ids.insert(*eid);
            if let Some(e) = get_element(grid, *eid) {
                installed_nodes.insert(e.node_i_id);
                installed_nodes.insert(e.node_j_id);
            }
        }

        members_added_last_300.push(step.element_ids.len());
        if members_added_last_300.len() > 300 {
            members_added_last_300.remove(0);
        }
        steps.push(step);

        if global_step_count >= 300 {
            let recent_sum: usize = members_added_last_300.iter().sum();
            if recent_sum < 3 {
                break TerminationReason::NoProgress;
            }
        }

        if global_step_count as usize >= total_elements * 10 + 1000 {
            break TerminationReason::MaxIterations;
        }
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
}
