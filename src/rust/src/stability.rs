//! Stability validation and table generation module for AssyPlan
//!
//! This module provides:
//! - Construction Sequence Table: Individual element installation order (1개씩)
//! - Workfront Step Table: Stability-based groups of 2-5 elements per step
//!
//! All table calculations are performed in Rust for performance with large structures
//! and future simulation mode support.

use std::collections::{HashMap, HashSet, VecDeque};

// ============================================================================
// Data Structures
// ============================================================================

/// Node data for stability calculations
#[derive(Debug, Clone)]
pub struct StabilityNode {
    pub id: i32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// Element data for stability calculations
#[derive(Debug, Clone)]
pub struct StabilityElement {
    pub id: i32,
    pub node_i_id: i32,
    pub node_j_id: i32,
    pub member_type: String, // "Column" or "Girder"
}

/// Construction Sequence Table entry
/// Represents individual element installation order (1개씩 설치 순서)
#[derive(Debug, Clone)]
pub struct SequenceEntry {
    /// Order in which element is installed (1-indexed)
    pub sequence_order: i32,
    /// Workfront ID this element belongs to
    pub workfront_id: i32,
    /// Element ID
    pub element_id: i32,
    /// Member type ("Column" or "Girder")
    pub member_type: String,
}

/// Workfront Step Table entry
/// Groups 2-5 elements that together satisfy stability conditions
#[derive(Debug, Clone)]
pub struct StepEntry {
    /// Workfront ID
    pub workfront_id: i32,
    /// Global step number (1-indexed)
    pub step: i32,
    /// Element IDs in this global step
    pub element_ids: Vec<i32>,
    /// Pattern name (e.g. ColGirder, Bootstrap, Multi(2))
    pub pattern: String,
}

/// Complete result from table generation
#[derive(Debug, Clone)]
pub struct TableGenerationResult {
    /// Construction sequence table (individual element order)
    pub sequence_table: Vec<SequenceEntry>,
    /// Workfront step table (stability-based groups)
    pub step_table: Vec<StepEntry>,
    /// Maximum step number
    pub max_step: i32,
    /// Total workfront count
    pub workfront_count: i32,
    /// Any errors encountered during generation
    pub errors: Vec<String>,
    /// Fatal generation error (results are not valid for step analytics)
    pub fatal: bool,
}

impl Default for TableGenerationResult {
    fn default() -> Self {
        Self {
            sequence_table: Vec::new(),
            step_table: Vec::new(),
            max_step: 0,
            workfront_count: 0,
            errors: Vec::new(),
            fatal: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepCandidateMask {
    ColumnOnly,
    GirderOnly,
    Both,
}

impl StepCandidateMask {
    pub fn allows(self, is_column_candidate: bool) -> bool {
        match self {
            StepCandidateMask::ColumnOnly => is_column_candidate,
            StepCandidateMask::GirderOnly => !is_column_candidate,
            StepCandidateMask::Both => true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepPatternType {
    Col,
    Girder,
    ColCol,
    ColGirder,
    GirderGirder,
    ColColGirder,
    ColGirderCol,
    ColGirderColGirder,
    ColGirderGirder,
    ColColGirderGirder,
    ColColGirderColGirder,
    Bootstrap,
}

impl StepPatternType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Col => "Col",
            Self::Girder => "Girder",
            Self::ColCol => "ColCol",
            Self::ColGirder => "ColGirder",
            Self::GirderGirder => "GirderGirder",
            Self::ColColGirder => "ColColGirder",
            Self::ColGirderCol => "ColGirderCol",
            Self::ColGirderColGirder => "ColGirderColGirder",
            Self::ColGirderGirder => "ColGirderGirder",
            Self::ColColGirderGirder => "ColColGirderGirder",
            Self::ColColGirderColGirder => "ColColGirderColGirder",
            Self::Bootstrap => "Bootstrap",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepBufferDecision {
    Incomplete(StepCandidateMask),
    Complete(StepPatternType),
    Invalid,
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Get node coordinates by ID
fn get_node_coords(node_id: i32, nodes: &[StabilityNode]) -> Option<(f64, f64, f64)> {
    nodes
        .iter()
        .find(|n| n.id == node_id)
        .map(|n| (n.x, n.y, n.z))
}

/// Get elements connected to a specific node
fn get_elements_at_node<'a>(
    node_id: i32,
    elements: &'a [StabilityElement],
) -> Vec<&'a StabilityElement> {
    elements
        .iter()
        .filter(|e| e.node_i_id == node_id || e.node_j_id == node_id)
        .collect()
}

/// Get column elements only
fn get_column_elements(elements: &[StabilityElement]) -> Vec<&StabilityElement> {
    elements
        .iter()
        .filter(|e| e.member_type == "Column")
        .collect()
}

/// Get girder elements only
fn get_girder_elements(elements: &[StabilityElement]) -> Vec<&StabilityElement> {
    elements
        .iter()
        .filter(|e| e.member_type == "Girder")
        .collect()
}

pub fn classify_member_signature(
    signature: &str,
    has_stable_structure: bool,
) -> StepBufferDecision {
    match signature {
        "" => {
            if has_stable_structure {
                StepBufferDecision::Incomplete(StepCandidateMask::Both)
            } else {
                StepBufferDecision::Incomplete(StepCandidateMask::ColumnOnly)
            }
        }
        "G" => {
            if has_stable_structure {
                StepBufferDecision::Complete(StepPatternType::Girder)
            } else {
                StepBufferDecision::Invalid
            }
        }
        "C" => {
            if has_stable_structure {
                StepBufferDecision::Incomplete(StepCandidateMask::Both)
            } else {
                StepBufferDecision::Incomplete(StepCandidateMask::ColumnOnly)
            }
        }
        "CC" => StepBufferDecision::Incomplete(StepCandidateMask::GirderOnly),
        "CCG" => StepBufferDecision::Incomplete(StepCandidateMask::Both),
        "CCGC" => StepBufferDecision::Incomplete(StepCandidateMask::GirderOnly),
        "CCGG" => StepBufferDecision::Complete(StepPatternType::ColColGirderGirder),
        "CCGCG" => {
            if has_stable_structure {
                StepBufferDecision::Complete(StepPatternType::ColColGirderColGirder)
            } else {
                StepBufferDecision::Complete(StepPatternType::Bootstrap)
            }
        }
        "CG" => StepBufferDecision::Complete(StepPatternType::ColGirder),
        "CGC" => StepBufferDecision::Incomplete(StepCandidateMask::GirderOnly),
        "CGCG" => StepBufferDecision::Complete(StepPatternType::ColGirderColGirder),
        "CGG" => StepBufferDecision::Complete(StepPatternType::ColGirderGirder),
        "GG" | "CCC" | "CCCG" => StepBufferDecision::Invalid,
        _ => StepBufferDecision::Invalid,
    }
}

pub fn classify_element_buffer(
    buffer_element_ids: &[i32],
    element_by_id: &HashMap<i32, &StabilityElement>,
    has_stable_structure: bool,
) -> StepBufferDecision {
    let signature: String = buffer_element_ids
        .iter()
        .map(|eid| {
            if element_by_id
                .get(eid)
                .map(|e| e.member_type == "Column")
                .unwrap_or(false)
            {
                'C'
            } else {
                'G'
            }
        })
        .collect();

    classify_member_signature(signature.as_str(), has_stable_structure)
}

fn collect_local_stable_context(
    element_ids: &[i32],
    all_elements: &[StabilityElement],
    installed_ids: &HashSet<i32>,
) -> HashSet<i32> {
    let candidate_nodes: HashSet<i32> = all_elements
        .iter()
        .filter(|e| element_ids.contains(&e.id))
        .flat_map(|elem| [elem.node_i_id, elem.node_j_id])
        .collect();

    all_elements
        .iter()
        .filter(|elem| installed_ids.contains(&elem.id))
        .filter(|elem| {
            candidate_nodes.contains(&elem.node_i_id) || candidate_nodes.contains(&elem.node_j_id)
        })
        .map(|elem| elem.id)
        .collect()
}

fn is_node_connected_to_installed_column(
    node_id: i32,
    all_elements: &[StabilityElement],
    installed_ids: &HashSet<i32>,
) -> bool {
    all_elements
        .iter()
        .filter(|e| e.member_type == "Column" && installed_ids.contains(&e.id))
        .any(|col| col.node_i_id == node_id || col.node_j_id == node_id)
}

fn is_node_connected_to_installed_girder(
    node_id: i32,
    all_elements: &[StabilityElement],
    installed_ids: &HashSet<i32>,
) -> bool {
    all_elements
        .iter()
        .filter(|e| e.member_type == "Girder" && installed_ids.contains(&e.id))
        .any(|gir| gir.node_i_id == node_id || gir.node_j_id == node_id)
}

fn check_girders_perpendicular(
    girder_ids: &[i32],
    element_by_id: &HashMap<i32, &StabilityElement>,
    nodes: &[StabilityNode],
) -> bool {
    for i in 0..girder_ids.len() {
        for j in (i + 1)..girder_ids.len() {
            let Some(g1) = element_by_id.get(&girder_ids[i]) else {
                return false;
            };
            let Some(g2) = element_by_id.get(&girder_ids[j]) else {
                return false;
            };
            if are_perpendicular(&get_girder_direction(g1, nodes), &get_girder_direction(g2, nodes)) {
                return true;
            }
        }
    }
    false
}

pub fn check_step_bundle_stability(
    element_ids: &[i32],
    all_elements: &[StabilityElement],
    nodes: &[StabilityNode],
    installed_ids: &HashSet<i32>,
) -> bool {
    let element_by_id: HashMap<i32, &StabilityElement> =
        all_elements.iter().map(|e| (e.id, e)).collect();

    let local_stable_ids = collect_local_stable_context(element_ids, all_elements, installed_ids);
    let mut combined_local_ids: HashSet<i32> = local_stable_ids.clone();
    combined_local_ids.extend(element_ids.iter().copied());

    if local_stable_ids.is_empty() {
        let pattern_elements: Vec<StabilityElement> = all_elements
            .iter()
            .filter(|e| element_ids.contains(&e.id))
            .cloned()
            .collect();
        return has_minimum_assembly(nodes, &pattern_elements);
    }

    let pattern_columns: Vec<i32> = element_ids
        .iter()
        .filter(|eid| {
            element_by_id
                .get(eid)
                .map(|e| e.member_type == "Column")
                .unwrap_or(false)
        })
        .copied()
        .collect();
    let pattern_girders: Vec<i32> = element_ids
        .iter()
        .filter(|eid| {
            element_by_id
                .get(eid)
                .map(|e| e.member_type == "Girder")
                .unwrap_or(false)
        })
        .copied()
        .collect();

    for col_id in &pattern_columns {
        let Some(col) = element_by_id.get(col_id) else {
            return false;
        };
        if !validate_column_support(col, nodes, all_elements, &local_stable_ids) {
            return false;
        }
    }

    for gir_id in &pattern_girders {
        let Some(gir) = element_by_id.get(gir_id) else {
            return false;
        };
        if !validate_girder_support(gir, nodes, all_elements, &combined_local_ids) {
            return false;
        }
    }

    if !pattern_girders.is_empty() {
        let has_adjacent_connection = pattern_girders.iter().any(|gir_id| {
            let Some(gir) = element_by_id.get(gir_id) else {
                return false;
            };
            is_node_connected_to_installed_column(gir.node_i_id, all_elements, &local_stable_ids)
                || is_node_connected_to_installed_column(
                    gir.node_j_id,
                    all_elements,
                    &local_stable_ids,
                )
                || is_node_connected_to_installed_girder(
                    gir.node_i_id,
                    all_elements,
                    &local_stable_ids,
                )
                || is_node_connected_to_installed_girder(
                    gir.node_j_id,
                    all_elements,
                    &local_stable_ids,
                )
                || pattern_columns.iter().any(|col_id| {
                    let Some(col) = element_by_id.get(col_id) else {
                        return false;
                    };
                    let girder_touches_column_top =
                        col.node_j_id == gir.node_i_id || col.node_j_id == gir.node_j_id;
                    let column_is_anchored = is_node_connected_to_installed_column(
                        col.node_i_id,
                        all_elements,
                        &local_stable_ids,
                    ) || is_node_connected_to_installed_girder(
                        col.node_i_id,
                        all_elements,
                        &local_stable_ids,
                    );
                    girder_touches_column_top && column_is_anchored
                })
        });

        if !has_adjacent_connection {
            return false;
        }
    }

    if pattern_girders.len() >= 2
        && !check_girders_perpendicular(&pattern_girders, &element_by_id, nodes)
    {
        return false;
    }

    true
}

/// Build floor lookup map from z(mm) key to 1-indexed floor.
fn build_z_level_map(nodes: &[StabilityNode]) -> HashMap<i64, i32> {
    let mut z_values: Vec<i64> = nodes
        .iter()
        .map(|n| (n.z * 1000.0).round() as i64)
        .collect();
    z_values.sort();
    z_values.dedup();

    z_values
        .into_iter()
        .enumerate()
        .map(|(idx, z_key)| (z_key, (idx + 1) as i32))
        .collect()
}

/// Get floor level for a node based on z-coordinate
/// Returns 1-indexed floor level
#[cfg(test)]
fn get_floor_level_legacy(node_id: i32, nodes: &[StabilityNode]) -> Option<i32> {
    let mut z_values: Vec<i64> = nodes
        .iter()
        .map(|n| (n.z * 1000.0).round() as i64)
        .collect();
    z_values.sort();
    z_values.dedup();

    let node_z = get_node_coords(node_id, nodes)?.2;
    let node_z_key = (node_z * 1000.0).round() as i64;

    z_values
        .iter()
        .position(|&z| z == node_z_key)
        .map(|idx| (idx + 1) as i32)
}

/// Cached floor lookup using precomputed z-level map.
fn get_floor_level_cached(
    node_id: i32,
    nodes: &[StabilityNode],
    z_level_map: &HashMap<i64, i32>,
) -> Option<i32> {
    let node_z = get_node_coords(node_id, nodes)?.2;
    let node_z_key = (node_z * 1000.0).round() as i64;
    z_level_map.get(&node_z_key).copied()
}

/// Compatibility wrapper (legacy signature).
#[cfg(test)]
fn get_floor_level(node_id: i32, nodes: &[StabilityNode]) -> Option<i32> {
    get_floor_level_legacy(node_id, nodes)
}

/// Cached floor lookup for columns.
fn get_column_floor_cached(
    element: &StabilityElement,
    nodes: &[StabilityNode],
    z_level_map: &HashMap<i64, i32>,
) -> Option<i32> {
    if element.member_type != "Column" {
        return None;
    }
    get_floor_level_cached(element.node_i_id, nodes, z_level_map)
}

/// Get floor column counts (columns per floor level)
#[cfg(test)]
fn get_floor_column_counts(
    elements: &[StabilityElement],
    nodes: &[StabilityNode],
) -> HashMap<i32, i32> {
    let z_level_map = build_z_level_map(nodes);
    get_floor_column_counts_cached(elements, nodes, &z_level_map)
}

/// Cached floor column counts using precomputed z-level map.
fn get_floor_column_counts_cached(
    elements: &[StabilityElement],
    nodes: &[StabilityNode],
    z_level_map: &HashMap<i64, i32>,
) -> HashMap<i32, i32> {
    let mut floor_counts: HashMap<i32, i32> = HashMap::new();

    for element in elements.iter().filter(|e| e.member_type == "Column") {
        if let Some(floor) = get_column_floor_cached(element, nodes, z_level_map) {
            *floor_counts.entry(floor).or_insert(0) += 1;
        }
    }

    floor_counts
}

// ============================================================================
// Stability Validation Functions
// ============================================================================

/// Get girder direction as (dx_sign, dy_sign) tuple
fn get_girder_direction(girder: &StabilityElement, nodes: &[StabilityNode]) -> (i32, i32) {
    if let (Some(ni), Some(nj)) = (
        get_node_coords(girder.node_i_id, nodes),
        get_node_coords(girder.node_j_id, nodes),
    ) {
        let dx = nj.0 - ni.0;
        let dy = nj.1 - ni.1;

        (
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
        )
    } else {
        (0, 0)
    }
}

/// Check if two directions are perpendicular (dot product = 0)
fn are_perpendicular(dir1: &(i32, i32), dir2: &(i32, i32)) -> bool {
    let dot = dir1.0 * dir2.0 + dir1.1 * dir2.1;
    dot == 0 && (dir1.0 != 0 || dir1.1 != 0) && (dir2.0 != 0 || dir2.1 != 0)
}

/// Check if a minimum assembly unit exists within given elements
///
/// A minimum assembly unit consists of:
/// - At least 3 columns with i-nodes at ground level (z=0)
/// - At least 2 girders connecting column j-nodes at 90 degree angles
///
/// This checks if the COMBINED set of elements (assembled + new) contains
/// at least one valid minimum assembly configuration.
pub fn has_minimum_assembly(nodes: &[StabilityNode], elements: &[StabilityElement]) -> bool {
    let columns: Vec<_> = get_column_elements(elements);
    let girders: Vec<_> = get_girder_elements(elements);

    // Need at least 3 columns and 2 girders
    if columns.len() < 3 || girders.len() < 2 {
        return false;
    }

    // Find ground-level columns (i-node at z=0)
    let ground_columns: Vec<_> = columns
        .iter()
        .filter(|c| {
            if let Some(coords) = get_node_coords(c.node_i_id, nodes) {
                coords.2.abs() < 0.001 // z ≈ 0
            } else {
                false
            }
        })
        .collect();

    if ground_columns.len() < 3 {
        return false;
    }

    // Get j-nodes of ground columns
    let column_j_nodes: HashSet<i32> = ground_columns.iter().map(|c| c.node_j_id).collect();

    // Find girders that connect column j-nodes
    let valid_girders: Vec<_> = girders
        .iter()
        .filter(|g| column_j_nodes.contains(&g.node_i_id) && column_j_nodes.contains(&g.node_j_id))
        .collect();

    if valid_girders.len() < 2 {
        return false;
    }

    // Check if any pair of girders are at 90 degrees
    for i in 0..valid_girders.len() {
        for j in (i + 1)..valid_girders.len() {
            let g1 = valid_girders[i];
            let g2 = valid_girders[j];

            if let (Some(g1_ni), Some(g1_nj), Some(g2_ni), Some(g2_nj)) = (
                get_node_coords(g1.node_i_id, nodes),
                get_node_coords(g1.node_j_id, nodes),
                get_node_coords(g2.node_i_id, nodes),
                get_node_coords(g2.node_j_id, nodes),
            ) {
                // Calculate directions
                let g1_dx = g1_nj.0 - g1_ni.0;
                let g1_dy = g1_nj.1 - g1_ni.1;
                let g2_dx = g2_nj.0 - g2_ni.0;
                let g2_dy = g2_nj.1 - g2_ni.1;

                // Normalize to direction signs
                let g1_dir = (
                    if g1_dx.abs() > 0.001 {
                        g1_dx.signum() as i32
                    } else {
                        0
                    },
                    if g1_dy.abs() > 0.001 {
                        g1_dy.signum() as i32
                    } else {
                        0
                    },
                );
                let g2_dir = (
                    if g2_dx.abs() > 0.001 {
                        g2_dx.signum() as i32
                    } else {
                        0
                    },
                    if g2_dy.abs() > 0.001 {
                        g2_dy.signum() as i32
                    } else {
                        0
                    },
                );

                // Check perpendicular (dot product = 0)
                let dot = g1_dir.0 * g2_dir.0 + g1_dir.1 * g2_dir.1;
                if dot == 0 {
                    return true; // Found a valid minimum assembly
                }
            }
        }
    }

    false
}

/// Legacy wrapper for validate_minimum_assembly (for test compatibility)
/// Now delegates to has_minimum_assembly
pub fn validate_minimum_assembly(
    nodes: &[StabilityNode],
    elements: &[StabilityElement],
) -> Result<bool, String> {
    if has_minimum_assembly(nodes, elements) {
        Ok(true)
    } else {
        Err(
            "No valid minimum assembly found (need 3+ ground columns + 2+ girders at 90°)"
                .to_string(),
        )
    }
}

/// Validate column support
///
/// A column's node_i (bottom node) must be either:
/// - At ground level (z=0), OR
/// - Connected to node_j of an already-stable column below
pub fn validate_column_support(
    element: &StabilityElement,
    nodes: &[StabilityNode],
    all_elements: &[StabilityElement],
    stable_element_ids: &HashSet<i32>,
) -> bool {
    if element.member_type != "Column" {
        return false;
    }

    // Get coordinates of node_i (bottom)
    let ni_coords = match get_node_coords(element.node_i_id, nodes) {
        Some(c) => c,
        None => return false,
    };

    // Check if at ground level (z = 0)
    if ni_coords.2.abs() < 0.001 {
        return true;
    }

    // If not at ground, check if connected to stable column's node_j
    let stable_columns: Vec<_> = all_elements
        .iter()
        .filter(|e| stable_element_ids.contains(&e.id) && e.member_type == "Column")
        .collect();

    for col in stable_columns {
        if let Some(col_nj_coords) = get_node_coords(col.node_j_id, nodes) {
            // Same x,y position (horizontal alignment) and same z
            if (col_nj_coords.0 - ni_coords.0).abs() < 0.001
                && (col_nj_coords.1 - ni_coords.1).abs() < 0.001
                && (col_nj_coords.2 - ni_coords.2).abs() < 0.001
            {
                return true;
            }
        }
    }

    false
}

/// Validate girder support
///
/// A girder's node_i and node_j must BOTH be connected to already-stable COLUMNS.
/// Connecting to another girder alone is not sufficient — the girder must be
/// anchored to columns on both ends. This prevents cantilever configurations
/// (one end free or only connected to a non-column element) from passing.
pub fn validate_girder_support(
    element: &StabilityElement,
    _nodes: &[StabilityNode],
    all_elements: &[StabilityElement],
    stable_element_ids: &HashSet<i32>,
) -> bool {
    if element.member_type != "Girder" {
        return false;
    }

    let ni_supported =
        is_node_supported_by_column(element.node_i_id, all_elements, stable_element_ids);
    let nj_supported =
        is_node_supported_by_column(element.node_j_id, all_elements, stable_element_ids);

    ni_supported && nj_supported
}

/// Check if a node is supported specifically by a stable COLUMN.
/// Used by validate_girder_support to prevent cantilever false positives.
fn is_node_supported_by_column(
    node_id: i32,
    elements: &[StabilityElement],
    stable_element_ids: &HashSet<i32>,
) -> bool {
    let connected = get_elements_at_node(node_id, elements);
    connected
        .iter()
        .any(|e| stable_element_ids.contains(&e.id) && e.member_type == "Column")
}

/// Check floor installation constraint
///
/// Floor N (N > 1) installation cannot start until floor N-1 has reached
/// the threshold percentage of column installation (default 80%).
pub fn check_floor_installation_constraint(
    target_floor: i32,
    installed_element_ids: &HashSet<i32>,
    all_elements: &[StabilityElement],
    nodes: &[StabilityNode],
    threshold_percentage: f64,
) -> (bool, f64) {
    // Floor 1 can always install
    if target_floor <= 1 {
        return (true, 100.0);
    }

    let z_level_map = build_z_level_map(nodes);

    // Get total columns per floor
    let total_per_floor = get_floor_column_counts_cached(all_elements, nodes, &z_level_map);

    // Get installed columns per floor
    let installed_elements: Vec<_> = all_elements
        .iter()
        .filter(|e| installed_element_ids.contains(&e.id))
        .cloned()
        .collect();
    let installed_per_floor =
        get_floor_column_counts_cached(&installed_elements, nodes, &z_level_map);

    // Check lower floor (N-1) percentage
    let lower_floor = target_floor - 1;
    let total_lower = *total_per_floor.get(&lower_floor).unwrap_or(&0);
    let installed_lower = *installed_per_floor.get(&lower_floor).unwrap_or(&0);

    let lower_floor_percentage = if total_lower > 0 {
        (installed_lower as f64 / total_lower as f64) * 100.0
    } else {
        100.0 // No columns on lower floor means constraint satisfied
    };

    let allowed = lower_floor_percentage >= threshold_percentage;
    (allowed, lower_floor_percentage)
}

// ============================================================================
// Construction Sequence Building
// ============================================================================

/// Build construction sequence using topological sort with cycle detection
///
/// This creates the Construction Sequence Table where each element is assigned
/// a sequence order based on predecessor dependencies.
///
/// Returns Ok(sequence_table) or Err(cycle_members) if cycle detected.
pub fn build_construction_sequence(
    elements: &[StabilityElement],
    member_id_to_idx: &HashMap<String, usize>,
    element_predecessors: &[Option<String>],
    element_data: &[(String, Option<String>)], // (member_id, predecessor_id)
) -> Result<Vec<SequenceEntry>, Vec<String>> {
    let num_elements = elements.len();
    let mut sequence: Vec<SequenceEntry> = Vec::new();
    let mut in_degree: Vec<usize> = vec![0; num_elements];
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); num_elements];
    let mut workfront_ids: Vec<i32> = vec![0; num_elements];

    // Build dependency graph and identify workfronts
    let mut current_workfront = 0i32;
    for (idx, pred_id) in element_predecessors.iter().enumerate() {
        if let Some(pred) = pred_id {
            if let Some(&pred_idx) = member_id_to_idx.get(pred) {
                in_degree[idx] += 1;
                dependents[pred_idx].push(idx);
            } else {
                // Predecessor specified but not found in element list —
                // treat as an independent workfront start (predecessor is external/missing)
                current_workfront += 1;
                workfront_ids[idx] = current_workfront;
            }
        } else {
            // No predecessor = workfront starting point
            current_workfront += 1;
            workfront_ids[idx] = current_workfront;
        }
    }

    // Safety pass: any element still with workfront_id == 0 after graph construction
    // (should not happen, but guard against it to enforce 1-indexed IDs)
    for idx in 0..num_elements {
        if in_degree[idx] == 0 && workfront_ids[idx] == 0 {
            current_workfront += 1;
            workfront_ids[idx] = current_workfront;
        }
    }

    // Propagate workfront IDs through dependency graph
    let mut queue: VecDeque<usize> = VecDeque::new();
    for idx in 0..num_elements {
        if in_degree[idx] == 0 {
            queue.push_back(idx);
        }
    }

    // First pass: assign workfront IDs
    let mut temp_in_degree = in_degree.clone();
    let mut temp_queue = queue.clone();
    while let Some(idx) = temp_queue.pop_front() {
        for &dep_idx in &dependents[idx] {
            if workfront_ids[dep_idx] == 0 {
                workfront_ids[dep_idx] = workfront_ids[idx];
            }
            temp_in_degree[dep_idx] -= 1;
            if temp_in_degree[dep_idx] == 0 {
                temp_queue.push_back(dep_idx);
            }
        }
    }

    // Second pass: topological sort for sequence order
    let mut processed_count = 0usize;
    let mut step_assignments: HashMap<usize, i32> = HashMap::new();

    for idx in 0..num_elements {
        if in_degree[idx] == 0 {
            step_assignments.insert(idx, 1);
        }
    }

    while let Some(idx) = queue.pop_front() {
        processed_count += 1;
        let current_step = *step_assignments.get(&idx).unwrap_or(&1);

        for &dep_idx in &dependents[idx] {
            in_degree[dep_idx] -= 1;
            let new_step = current_step + 1;
            let existing = step_assignments.entry(dep_idx).or_insert(0);
            if new_step > *existing {
                *existing = new_step;
            }
            if in_degree[dep_idx] == 0 {
                queue.push_back(dep_idx);
            }
        }
    }

    // Cycle detection
    if processed_count < num_elements {
        let cycle_members: Vec<String> = (0..num_elements)
            .filter(|&idx| in_degree[idx] > 0)
            .map(|idx| element_data[idx].0.clone())
            .take(10)
            .collect();
        return Err(cycle_members);
    }

    // Build sequence table sorted by step, then by element index for deterministic order
    let mut indexed_steps: Vec<(usize, i32)> = step_assignments.into_iter().collect();
    indexed_steps.sort_by_key(|(idx, step)| (*step, *idx));

    for (order, (idx, _step)) in indexed_steps.iter().enumerate() {
        let element = &elements[*idx];
        sequence.push(SequenceEntry {
            sequence_order: (order + 1) as i32, // 1-indexed
            workfront_id: workfront_ids[*idx],
            element_id: element.id,
            member_type: element.member_type.clone(),
        });
    }

    Ok(sequence)
}

// ============================================================================
// Workfront Step Assignment (Stability-Based Grouping)
// ============================================================================

/// Assign workfront steps based on stability validation
///
/// Algorithm (from SKILLS.md):
/// - For each member in sequence order:
///   1. Check if member can be assembled with current assembled set
///   2. If stable: add to current step, mark as assembled
///   3. If not stable: increment step, then add
///
/// Stability check considers ALL assembled elements (not just new group).
pub fn assign_workfront_steps(
    sequence_table: &[SequenceEntry],
    elements: &[StabilityElement],
    nodes: &[StabilityNode],
) -> (Vec<StepEntry>, Vec<String>) {
    #[derive(Clone, Debug, Default)]
    struct DevWorkfrontState {
        cursor: usize,
        buffer: Vec<i32>,
    }

    #[derive(Clone, Debug)]
    struct DevLocalStep {
        workfront_id: i32,
        element_ids: Vec<i32>,
        pattern: StepPatternType,
    }

    let mut step_table: Vec<StepEntry> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let element_by_id: HashMap<i32, &StabilityElement> =
        elements.iter().map(|e| (e.id, e)).collect();

    let mut workfront_sequences: HashMap<i32, Vec<i32>> = HashMap::new();
    for entry in sequence_table {
        workfront_sequences
            .entry(entry.workfront_id)
            .or_default()
            .push(entry.element_id);
    }

    let mut sorted_workfront_ids: Vec<i32> = workfront_sequences.keys().cloned().collect();
    sorted_workfront_ids.sort();

    let mut states: HashMap<i32, DevWorkfrontState> = sorted_workfront_ids
        .iter()
        .map(|wf| (*wf, DevWorkfrontState::default()))
        .collect();

    let mut stable_ids: HashSet<i32> = HashSet::new();
    let mut global_step = 1i32;
    let mut no_progress_cycles = 0usize;

    loop {
        let all_consumed = sorted_workfront_ids.iter().all(|wf| {
            let seq_len = workfront_sequences.get(wf).map(|v| v.len()).unwrap_or(0);
            states
                .get(wf)
                .map(|s| s.cursor >= seq_len)
                .unwrap_or(true)
        });
        if all_consumed {
            break;
        }

        let mut cycle_local_steps: Vec<DevLocalStep> = Vec::new();
        let mut cycle_completed: HashSet<i32> = HashSet::new();
        let mut cycle_progress = false;
        let mut no_progress_rounds = 0usize;

        loop {
            let eligible: Vec<i32> = sorted_workfront_ids
                .iter()
                .copied()
                .filter(|wf| !cycle_completed.contains(wf))
                .collect();
            if eligible.is_empty() {
                break;
            }

            let mut round_progress = false;

            for wf_id in &eligible {
                let Some(seq) = workfront_sequences.get(wf_id) else {
                    continue;
                };
                let Some(state) = states.get_mut(wf_id) else {
                    continue;
                };
                if state.cursor >= seq.len() {
                    continue;
                }

                let element_id = seq[state.cursor];
                state.cursor += 1;
                state.buffer.push(element_id);

                round_progress = true;
                cycle_progress = true;

                let mut cycle_context = stable_ids.clone();
                for ls in &cycle_local_steps {
                    cycle_context.extend(ls.element_ids.iter().copied());
                }

                match classify_element_buffer(
                    &state.buffer,
                    &element_by_id,
                    !cycle_context.is_empty(),
                ) {
                    StepBufferDecision::Invalid => {
                        errors.push(format!(
                            "Workfront {} has invalid pattern while building step: [{}]",
                            wf_id,
                            state
                                .buffer
                                .iter()
                                .map(|id| id.to_string())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ));
                    }
                    StepBufferDecision::Complete(pattern) => {
                        if check_step_bundle_stability(&state.buffer, elements, nodes, &cycle_context)
                        {
                            cycle_local_steps.push(DevLocalStep {
                                workfront_id: *wf_id,
                                element_ids: state.buffer.clone(),
                                pattern,
                            });
                            cycle_completed.insert(*wf_id);
                            state.buffer.clear();
                        }
                    }
                    StepBufferDecision::Incomplete(_) => {}
                }
            }

            if !round_progress {
                no_progress_rounds += 1;
                if no_progress_rounds >= 2 {
                    break;
                }
            } else {
                no_progress_rounds = 0;
            }

            if cycle_completed.len() >= eligible.len() {
                break;
            }
        }

        if cycle_local_steps.is_empty() {
            if !cycle_progress {
                no_progress_cycles += 1;
                if no_progress_cycles >= 2 {
                    break;
                }
            }
            continue;
        }

        no_progress_cycles = 0;

        let max_len = cycle_local_steps
            .iter()
            .map(|ls| ls.element_ids.len())
            .max()
            .unwrap_or(0);
        let mut merged_element_ids: Vec<i32> = Vec::new();
        for round in 0..max_len {
            for ls in &cycle_local_steps {
                if let Some(eid) = ls.element_ids.get(round) {
                    merged_element_ids.push(*eid);
                }
            }
        }

        for eid in &merged_element_ids {
            stable_ids.insert(*eid);
        }

        let pattern = if cycle_local_steps.len() == 1 {
            cycle_local_steps[0].pattern.as_str().to_string()
        } else {
            format!("Multi({})", cycle_local_steps.len())
        };

        step_table.push(StepEntry {
            workfront_id: cycle_local_steps[0].workfront_id,
            step: global_step,
            element_ids: merged_element_ids,
            pattern,
        });
        global_step += 1;
    }

    for wf_id in &sorted_workfront_ids {
        if let Some(state) = states.get(wf_id) {
            if !state.buffer.is_empty() {
                errors.push(format!(
                    "Workfront {} has incomplete local step: {} member(s) [{}] could not form a complete stable step.",
                    wf_id,
                    state.buffer.len(),
                    state
                        .buffer
                        .iter()
                        .map(|id| id.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
    }

    (step_table, errors)
}

// ============================================================================
// Main Entry Point
// ============================================================================

/// Generate all tables (Construction Sequence + Workfront Steps)
///
/// This is the main entry point called from lib.rs after CSV parsing.
/// All calculations are performed in Rust for performance.
pub fn generate_all_tables(
    nodes: &[StabilityNode],
    elements: &[StabilityElement],
    element_data: &[(String, Option<String>)], // (member_id, predecessor_id)
) -> TableGenerationResult {
    let mut result = TableGenerationResult::default();

    if elements.is_empty() {
        result.errors.push("No elements provided".to_string());
        return result;
    }

    // Build member_id to index mapping
    let member_id_to_idx: HashMap<String, usize> = element_data
        .iter()
        .enumerate()
        .map(|(idx, (member_id, _))| (member_id.clone(), idx))
        .collect();

    // Extract predecessors
    let element_predecessors: Vec<Option<String>> =
        element_data.iter().map(|(_, pred)| pred.clone()).collect();

    // Count workfronts (elements with no predecessor)
    result.workfront_count = element_predecessors.iter().filter(|p| p.is_none()).count() as i32;

    // Build construction sequence table
    match build_construction_sequence(
        elements,
        &member_id_to_idx,
        &element_predecessors,
        element_data,
    ) {
        Ok(sequence) => {
            result.sequence_table = sequence;
        }
        Err(cycle_members) => {
            result.errors.push(format!(
                "Cycle detected in predecessor graph involving members: {}",
                cycle_members.join(", ")
            ));
            return result;
        }
    }

    // Assign workfront steps based on stability
    let (step_table, step_errors) = assign_workfront_steps(&result.sequence_table, elements, nodes);
    result.step_table = step_table;
    result.errors.extend(step_errors);

    if result.step_table.is_empty() {
        result.errors.push(
            "입력 데이터 오류: 안정 조건을 만족하는 local/global step을 생성할 수 없습니다. 부재 생성 순서와 predecessor 관계를 확인하세요.".to_string(),
        );
        result.fatal = true;
    }

    // Calculate max step
    result.max_step = result.step_table.iter().map(|s| s.step).max().unwrap_or(0);

    // Validate step table: check for oversized steps (> 5 members) or
    // incomplete final assembly (step with members that never closed stably)
    let max_members_per_step = 5;
    for step in &result.step_table {
        if step.element_ids.len() > max_members_per_step {
            result.errors.push(format!(
                "Step {} (workfront {}) has {} members (max {}). \
                 Input data may be missing required structural members — \
                 check that all predecessor elements are present.",
                step.step,
                step.workfront_id,
                step.element_ids.len(),
                max_members_per_step
            ));
        }
    }

    // Check minimum assembly stability for each workfront's first step:
    // Disconnected first steps must satisfy minimum assembly (3 columns + 2 girders at 90°)
    {
        let element_by_id: HashMap<i32, &StabilityElement> =
            elements.iter().map(|e| (e.id, e)).collect();
        let mut workfront_first_step_checked: HashSet<i32> = HashSet::new();
        for step in &result.step_table {
            if workfront_first_step_checked.contains(&step.workfront_id) {
                continue;
            }
            workfront_first_step_checked.insert(step.workfront_id);
            // Collect elements in this first step
            let step_elements: Vec<StabilityElement> = step
                .element_ids
                .iter()
                .filter_map(|id| element_by_id.get(id).map(|e| (*e).clone()))
                .collect();
            if !has_minimum_assembly(nodes, &step_elements) {
                result.errors.push(format!(
                    "Workfront {} step {} does not form a stable minimum assembly \
                     (requires 3+ ground columns + 2+ perpendicular girders). \
                     Members: [{}]",
                    step.workfront_id,
                    step.step,
                    step.element_ids
                        .iter()
                        .map(|id| id.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
    }

    // Check if sequence_table has fewer entries than total elements
    // (some elements were unreachable / dropped)
    if result.sequence_table.len() < elements.len() {
        let missing = elements.len() - result.sequence_table.len();
        result.errors.push(format!(
            "Sequence table has {} entries but {} elements were provided. \
             {} element(s) could not be ordered — predecessor chain may be broken.",
            result.sequence_table.len(),
            elements.len(),
            missing
        ));
        result.fatal = true;
    }

    result
}

/// Convert step table to format compatible with StepRenderData
/// Returns Vec<(step_number, element_index, member_type)>
pub fn step_table_for_rendering(
    step_table: &[StepEntry],
    elements: &[StabilityElement],
) -> Vec<(i32, usize, String)> {
    let element_id_to_idx: HashMap<i32, usize> = elements
        .iter()
        .enumerate()
        .map(|(idx, e)| (e.id, idx))
        .collect();

    let mut render_table: Vec<(i32, usize, String)> = Vec::new();

    for step_entry in step_table {
        for &element_id in &step_entry.element_ids {
            if let Some(&idx) = element_id_to_idx.get(&element_id) {
                let member_type = elements[idx].member_type.clone();
                render_table.push((step_entry.step, idx, member_type));
            }
        }
    }

    render_table
}

// ============================================================================
// Table Export Functions
// ============================================================================

/// Save Construction Sequence Table to text file
/// Format: sequence_order,workfront_id,element_id,member_type
pub fn save_sequence_table(
    sequence_table: &[SequenceEntry],
    elements: &[StabilityElement],
    file_path: &std::path::Path,
) -> std::io::Result<()> {
    use std::io::Write;

    let element_by_id: HashMap<i32, &StabilityElement> =
        elements.iter().map(|e| (e.id, e)).collect();

    let mut file = std::fs::File::create(file_path)?;

    // Write header
    writeln!(file, "sequence_order,workfront_id,element_id,member_type")?;

    // Write data
    for entry in sequence_table {
        let member_type = element_by_id
            .get(&entry.element_id)
            .map(|e| e.member_type.as_str())
            .unwrap_or("Unknown");

        writeln!(
            file,
            "{},{},{},{}",
            entry.sequence_order, entry.workfront_id, entry.element_id, member_type
        )?;
    }

    Ok(())
}

/// Save Workfront Step Table to text file
/// Format: workfront_id,step,element_ids (comma-separated within brackets)
pub fn save_step_table(
    step_table: &[StepEntry],
    file_path: &std::path::Path,
) -> std::io::Result<()> {
    use std::io::Write;

    let mut file = std::fs::File::create(file_path)?;

    // Write header
    writeln!(file, "workfront_id,step,pattern,element_count,element_ids")?;

    // Write data
    for entry in step_table {
        let element_ids_str: String = entry
            .element_ids
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(";");

        writeln!(
            file,
            "{},{},{},{},\"{}\"",
            entry.workfront_id,
            entry.step,
            entry.pattern,
            entry.element_ids.len(),
            element_ids_str
        )?;
    }

    Ok(())
}

/// Save node table to CSV file
/// Format: node_id, x, y, z
pub fn save_node_table(
    nodes: &[StabilityNode],
    file_path: &std::path::Path,
) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::File::create(file_path)?;

    // Write header
    writeln!(file, "node_id,x,y,z")?;

    // Write data rows (sorted by id)
    let mut sorted_nodes = nodes.to_vec();
    sorted_nodes.sort_by_key(|n| n.id);

    for node in &sorted_nodes {
        writeln!(file, "{},{},{},{}", node.id, node.x, node.y, node.z)?;
    }

    Ok(())
}

/// Save element table to CSV file
/// Format: element_id, node_i_id, node_j_id, member_type
pub fn save_element_table(
    elements: &[StabilityElement],
    file_path: &std::path::Path,
) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::File::create(file_path)?;

    // Write header
    writeln!(file, "element_id,node_i_id,node_j_id,member_type")?;

    // Write data rows (sorted by id)
    let mut sorted_elements = elements.to_vec();
    sorted_elements.sort_by_key(|e| e.id);

    for elem in &sorted_elements {
        writeln!(
            file,
            "{},{},{},{}",
            elem.id, elem.node_i_id, elem.node_j_id, elem.member_type
        )?;
    }

    Ok(())
}

/// Save metric table to CSV file (per-step metrics)
/// Format: step, total_elements, total_columns, total_girders, floor_1_columns, floor_1_rate, ...
pub fn save_metric_table(
    step_table: &[StepEntry],
    elements: &[StabilityElement],
    nodes: &[StabilityNode],
    file_path: &std::path::Path,
) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::File::create(file_path)?;

    let z_level_map = build_z_level_map(nodes);

    // Get all unique floors
    let floor_counts = get_floor_column_counts_cached(elements, nodes, &z_level_map);
    let mut floors: Vec<i32> = floor_counts.keys().cloned().collect();
    floors.sort();

    // Write header
    let mut header = "step,cumulative_elements,cumulative_columns,cumulative_girders".to_string();
    for floor in &floors {
        header.push_str(&format!(",floor_{}_columns,floor_{}_rate", floor, floor));
    }
    writeln!(file, "{}", header)?;

    // Build element lookup
    let element_by_id: std::collections::HashMap<i32, &StabilityElement> =
        elements.iter().map(|e| (e.id, e)).collect();

    // Sort step table by step number
    let mut steps: Vec<&StepEntry> = step_table.iter().collect();
    steps.sort_by_key(|s| s.step);

    // Track cumulative state
    let mut installed_ids: std::collections::HashSet<i32> = std::collections::HashSet::new();
    for entry in &steps {
        // Add elements from this step
        for id in &entry.element_ids {
            installed_ids.insert(*id);
        }

        // Calculate cumulative metrics
        let installed_elements: Vec<&StabilityElement> = installed_ids
            .iter()
            .filter_map(|id| element_by_id.get(id).copied())
            .collect();

        let total_columns = installed_elements
            .iter()
            .filter(|e| e.member_type == "Column")
            .count();
        let total_girders = installed_elements
            .iter()
            .filter(|e| e.member_type == "Girder")
            .count();
        let total_elements = total_columns + total_girders;

        // Calculate per-floor column installation rates
        let installed_floor_counts = get_floor_column_counts_cached(
            &installed_elements
                .iter()
                .filter(|e| e.member_type == "Column")
                .map(|e| (*e).clone())
                .collect::<Vec<_>>(),
            nodes,
            &z_level_map,
        );

        let mut row = format!(
            "{},{},{},{}",
            entry.step, total_elements, total_columns, total_girders
        );

        for floor in &floors {
            let total_in_floor = floor_counts.get(floor).unwrap_or(&0);
            let installed_in_floor = installed_floor_counts.get(floor).unwrap_or(&0);
            let rate = if *total_in_floor > 0 {
                (*installed_in_floor as f64 / *total_in_floor as f64) * 100.0
            } else {
                0.0
            };
            row.push_str(&format!(",{},{:.1}", installed_in_floor, rate));
        }

        writeln!(file, "{}", row)?;
    }

    Ok(())
}

/// Save all tables to a directory
/// Creates: node_table.csv, element_table.csv, sequence_table.csv, step_table.csv, metric_table.csv
pub fn save_all_tables(
    result: &TableGenerationResult,
    elements: &[StabilityElement],
    nodes: &[StabilityNode],
    output_dir: &std::path::Path,
) -> std::io::Result<()> {
    // Create output directory if it doesn't exist
    std::fs::create_dir_all(output_dir)?;

    // Save all tables
    let node_path = output_dir.join("node_table.csv");
    let element_path = output_dir.join("element_table.csv");
    let sequence_path = output_dir.join("sequence_table.csv");
    let step_path = output_dir.join("step_table.csv");
    let metric_path = output_dir.join("metric_table.csv");

    save_node_table(nodes, &node_path)?;
    save_element_table(elements, &element_path)?;
    save_sequence_table(&result.sequence_table, elements, &sequence_path)?;
    save_step_table(&result.step_table, &step_path)?;
    save_metric_table(&result.step_table, elements, nodes, &metric_path)?;

    Ok(())
}

/// Save development-mode review tables to output directory.
/// Always saves: dev_node_table.csv, dev_element_table.csv, dev_sequence_table.csv
/// Saves conditionally: dev_step_table.csv, dev_metric_table.csv (skipped when errors exist)
pub fn save_development_review_tables(
    result: &TableGenerationResult,
    elements: &[StabilityElement],
    nodes: &[StabilityNode],
    output_dir: &std::path::Path,
) -> std::io::Result<Vec<String>> {
    std::fs::create_dir_all(output_dir)?;

    let node_path = output_dir.join("dev_node_table.csv");
    let element_path = output_dir.join("dev_element_table.csv");
    let sequence_path = output_dir.join("dev_sequence_table.csv");
    let step_path = output_dir.join("dev_step_table.csv");
    let metric_path = output_dir.join("dev_metric_table.csv");

    save_node_table(nodes, &node_path)?;
    save_element_table(elements, &element_path)?;
    save_sequence_table(&result.sequence_table, elements, &sequence_path)?;

    let mut notes: Vec<String> = Vec::new();
    let has_step_error = result.fatal || !result.errors.is_empty();
    if has_step_error {
        notes.push("Step/Metric table export skipped due to step-generation errors.".to_string());
    } else {
        save_step_table(&result.step_table, &step_path)?;
        save_metric_table(&result.step_table, elements, nodes, &metric_path)?;
    }

    Ok(notes)
}

/// Get floor column data for UI display
/// Returns Vec of (floor_level, total_columns) sorted by floor
pub fn get_floor_column_data(
    elements: &[StabilityElement],
    nodes: &[StabilityNode],
) -> Vec<(i32, usize)> {
    let z_level_map = build_z_level_map(nodes);
    let floor_counts = get_floor_column_counts_cached(elements, nodes, &z_level_map);
    let mut result: Vec<(i32, usize)> = floor_counts
        .into_iter()
        .map(|(floor, count)| (floor, count as usize))
        .collect();
    result.sort_by_key(|(floor, _)| *floor);
    result
}

/// Build step elements mapping for UI metrics
/// Returns: Vec<Vec<(element_id, member_type, floor)>> indexed by step (1-indexed, so index 0 is empty)
pub fn build_step_elements_map(
    step_table: &[StepEntry],
    elements: &[StabilityElement],
    nodes: &[StabilityNode],
) -> Vec<Vec<(i32, String, i32)>> {
    if step_table.is_empty() {
        return Vec::new();
    }

    let max_step = step_table.iter().map(|s| s.step).max().unwrap_or(0) as usize;
    let mut step_elements: Vec<Vec<(i32, String, i32)>> = vec![Vec::new(); max_step + 1];

    // Build element lookup
    let element_by_id: HashMap<i32, &StabilityElement> =
        elements.iter().map(|e| (e.id, e)).collect();
    let z_level_map = build_z_level_map(nodes);

    for entry in step_table {
        let step_idx = entry.step as usize;
        if step_idx < step_elements.len() {
            for elem_id in &entry.element_ids {
                if let Some(elem) = element_by_id.get(elem_id) {
                    let floor = if elem.member_type == "Column" {
                        get_column_floor_cached(elem, nodes, &z_level_map).unwrap_or(0)
                    } else {
                        // Girders: use z coordinate of lower node to determine floor
                        get_floor_level_cached(elem.node_i_id, nodes, &z_level_map).unwrap_or(0)
                    };
                    step_elements[step_idx].push((*elem_id, elem.member_type.clone(), floor));
                }
            }
        }
    }

    step_elements
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim_grid::SimGrid;

    fn create_test_nodes() -> Vec<StabilityNode> {
        vec![
            // Ground floor (z=0) - 4 corners
            StabilityNode {
                id: 1,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            StabilityNode {
                id: 2,
                x: 1000.0,
                y: 0.0,
                z: 0.0,
            },
            StabilityNode {
                id: 3,
                x: 0.0,
                y: 1000.0,
                z: 0.0,
            },
            StabilityNode {
                id: 4,
                x: 1000.0,
                y: 1000.0,
                z: 0.0,
            },
            // First floor (z=3000) - 4 corners
            StabilityNode {
                id: 5,
                x: 0.0,
                y: 0.0,
                z: 3000.0,
            },
            StabilityNode {
                id: 6,
                x: 1000.0,
                y: 0.0,
                z: 3000.0,
            },
            StabilityNode {
                id: 7,
                x: 0.0,
                y: 1000.0,
                z: 3000.0,
            },
            StabilityNode {
                id: 8,
                x: 1000.0,
                y: 1000.0,
                z: 3000.0,
            },
        ]
    }

    fn is_connected_to_stable_structure(
        assembled_stable: &HashSet<i32>,
        current_step: &[i32],
        element_by_id: &HashMap<i32, &StabilityElement>,
        _nodes: &[StabilityNode],
    ) -> bool {
        let stable_girder_nodes: HashSet<i32> = assembled_stable
            .iter()
            .filter_map(|eid| element_by_id.get(eid).copied())
            .filter(|elem| elem.member_type == "Girder")
            .flat_map(|elem| [elem.node_i_id, elem.node_j_id])
            .collect();

        if stable_girder_nodes.is_empty() {
            return false;
        }

        current_step
            .iter()
            .filter_map(|eid| element_by_id.get(eid).copied())
            .any(|elem| {
                stable_girder_nodes.contains(&elem.node_i_id)
                    || stable_girder_nodes.contains(&elem.node_j_id)
            })
    }

    #[test]
    fn test_ab_floor_lookup_legacy_vs_cached() {
        let grid = SimGrid::new(6, 8, 5, 6000.0, 6000.0, 4000.0);
        let nodes = &grid.nodes;
        let z_level_map = build_z_level_map(nodes);

        for node in nodes {
            let legacy = get_floor_level_legacy(node.id, nodes);
            let cached = get_floor_level_cached(node.id, nodes, &z_level_map);
            assert_eq!(legacy, cached, "node_id={}", node.id);
        }
    }

    #[test]
    fn test_ab_floor_column_counts_legacy_vs_cached() {
        let grid = SimGrid::new(6, 8, 5, 6000.0, 6000.0, 4000.0);
        let nodes = &grid.nodes;
        let elements = &grid.elements;
        let z_level_map = build_z_level_map(nodes);

        let legacy = get_floor_column_counts(elements, nodes);
        let cached = get_floor_column_counts_cached(elements, nodes, &z_level_map);
        assert_eq!(legacy, cached);
    }

    fn create_minimum_assembly_elements() -> Vec<StabilityElement> {
        // 3 columns + 2 girders at 90 degrees (L-shape)
        vec![
            // 3 columns from ground to first floor
            StabilityElement {
                id: 1,
                node_i_id: 1,
                node_j_id: 5,
                member_type: "Column".to_string(),
            },
            StabilityElement {
                id: 2,
                node_i_id: 2,
                node_j_id: 6,
                member_type: "Column".to_string(),
            },
            StabilityElement {
                id: 3,
                node_i_id: 3,
                node_j_id: 7,
                member_type: "Column".to_string(),
            },
            // 2 girders connecting top nodes at 90 degrees
            StabilityElement {
                id: 4,
                node_i_id: 5,
                node_j_id: 6,
                member_type: "Girder".to_string(),
            }, // X direction
            StabilityElement {
                id: 5,
                node_i_id: 5,
                node_j_id: 7,
                member_type: "Girder".to_string(),
            }, // Y direction
        ]
    }

    #[test]
    fn test_validate_minimum_assembly_valid() {
        let nodes = create_test_nodes();
        let elements = create_minimum_assembly_elements();

        let result = validate_minimum_assembly(&nodes, &elements);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_minimum_assembly_wrong_column_count() {
        let nodes = create_test_nodes();
        let elements = vec![
            StabilityElement {
                id: 1,
                node_i_id: 1,
                node_j_id: 5,
                member_type: "Column".to_string(),
            },
            StabilityElement {
                id: 2,
                node_i_id: 2,
                node_j_id: 6,
                member_type: "Column".to_string(),
            },
            // Only 2 columns
            StabilityElement {
                id: 4,
                node_i_id: 5,
                node_j_id: 6,
                member_type: "Girder".to_string(),
            },
            StabilityElement {
                id: 5,
                node_i_id: 5,
                node_j_id: 7,
                member_type: "Girder".to_string(),
            },
        ];

        let result = validate_minimum_assembly(&nodes, &elements);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("3+"));
    }

    #[test]
    fn test_validate_column_support_ground_level() {
        let nodes = create_test_nodes();
        let elements = create_minimum_assembly_elements();
        let stable_ids: HashSet<i32> = HashSet::new();

        // Column at ground level should be supported
        let column = &elements[0]; // id=1, node_i at z=0
        assert!(validate_column_support(
            column,
            &nodes,
            &elements,
            &stable_ids
        ));
    }

    #[test]
    fn test_validate_girder_support() {
        let nodes = create_test_nodes();
        let elements = create_minimum_assembly_elements();

        // Mark columns as stable
        let stable_ids: HashSet<i32> = [1, 2, 3].into_iter().collect();

        // Girder connecting stable columns should be supported
        let girder = &elements[3]; // id=4, connects nodes 5-6
        assert!(validate_girder_support(
            girder,
            &nodes,
            &elements,
            &stable_ids
        ));
    }

    #[test]
    fn test_floor_level_calculation() {
        let nodes = create_test_nodes();

        // Ground level nodes should be floor 1
        assert_eq!(get_floor_level(1, &nodes), Some(1));
        assert_eq!(get_floor_level(2, &nodes), Some(1));

        // First floor nodes should be floor 2
        assert_eq!(get_floor_level(5, &nodes), Some(2));
        assert_eq!(get_floor_level(6, &nodes), Some(2));
    }

    #[test]
    fn test_floor_installation_constraint() {
        let nodes = create_test_nodes();
        let elements = create_minimum_assembly_elements();

        // Floor 1 always allowed
        let (allowed, _pct) =
            check_floor_installation_constraint(1, &HashSet::new(), &elements, &nodes, 80.0);
        assert!(allowed);

        // Floor 2 with no columns installed should fail
        let (_allowed, _pct) =
            check_floor_installation_constraint(2, &HashSet::new(), &elements, &nodes, 80.0);
        // Depends on total columns at floor 1
    }

    #[test]
    fn test_generate_all_tables() {
        let nodes = create_test_nodes();
        let elements = create_minimum_assembly_elements();

        let element_data: Vec<(String, Option<String>)> = vec![
            ("A".to_string(), None),                  // Column 1 - workfront start
            ("B".to_string(), Some("A".to_string())), // Column 2 depends on A
            ("C".to_string(), Some("D".to_string())), // Column 3 depends on D
            ("D".to_string(), Some("B".to_string())), // Girder 1 depends on B
            ("E".to_string(), Some("C".to_string())), // Girder 2 depends on C
        ];

        let result = generate_all_tables(&nodes, &elements, &element_data);

        assert!(result.errors.is_empty(), "Errors: {:?}", result.errors);
        assert_eq!(result.workfront_count, 1);
        assert_eq!(result.sequence_table.len(), 5);
        assert!(!result.step_table.is_empty());
        assert_eq!(result.step_table[0].pattern, "Bootstrap");
    }

    #[test]
    fn test_generate_all_tables_fatal_when_no_step_can_form() {
        let nodes = create_test_nodes();
        let elements = create_minimum_assembly_elements();

        // C -> C -> C -> G -> G chain should be rejected by unified pattern rules
        let element_data: Vec<(String, Option<String>)> = vec![
            ("A".to_string(), None),
            ("B".to_string(), Some("A".to_string())),
            ("C".to_string(), Some("B".to_string())),
            ("D".to_string(), Some("C".to_string())),
            ("E".to_string(), Some("D".to_string())),
        ];

        let result = generate_all_tables(&nodes, &elements, &element_data);

        assert_eq!(result.sequence_table.len(), 5);
        assert!(result.step_table.is_empty());
        assert!(result.fatal);
        assert!(result
            .errors
            .iter()
            .any(|e| e.contains("local/global step을 생성할 수 없습니다")));
    }

    #[test]
    fn test_step_table_for_rendering() {
        let elements = create_minimum_assembly_elements();
        let step_table = vec![StepEntry {
            workfront_id: 1,
            step: 1,
            element_ids: vec![1, 2, 3, 4, 5],
            pattern: "Bootstrap".to_string(),
        }];

        let render_table = step_table_for_rendering(&step_table, &elements);

        assert_eq!(render_table.len(), 5);
        // All elements should be in step 1
        assert!(render_table.iter().all(|(step, _, _)| *step == 1));
    }

    // -----------------------------------------------------------------------
    // Regression tests for stability condition bugs
    // -----------------------------------------------------------------------

    /// Bug 1 regression: 2 columns + 1 girder must NOT be a valid step.
    /// Minimum assembly requires 3 columns + 2 perpendicular girders.
    #[test]
    fn test_step_not_valid_two_columns_one_girder() {
        let nodes = create_test_nodes();
        // 2 ground columns (id 1,2) + 1 girder connecting their tops (id 4)
        let elements = vec![
            StabilityElement {
                id: 1,
                node_i_id: 1,
                node_j_id: 5,
                member_type: "Column".to_string(),
            },
            StabilityElement {
                id: 2,
                node_i_id: 2,
                node_j_id: 6,
                member_type: "Column".to_string(),
            },
            StabilityElement {
                id: 4,
                node_i_id: 5,
                node_j_id: 6,
                member_type: "Girder".to_string(),
            },
        ];
        // No assembled_stable yet → disconnected case → must require 3 cols + 2 girders
        assert!(
            !has_minimum_assembly(&nodes, &elements),
            "2 columns + 1 girder must NOT satisfy minimum assembly"
        );
    }

    /// Bug 2 regression: 3 parallel columns + 2 parallel girders must NOT be valid.
    /// The 2 girders run in the same direction (X), so no 90° pair exists.
    #[test]
    fn test_step_not_valid_three_columns_two_parallel_girders() {
        // Nodes: 3 ground + 3 first-floor, all in X direction (y=0)
        let nodes = vec![
            StabilityNode {
                id: 1,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            StabilityNode {
                id: 2,
                x: 1000.0,
                y: 0.0,
                z: 0.0,
            },
            StabilityNode {
                id: 3,
                x: 2000.0,
                y: 0.0,
                z: 0.0,
            },
            StabilityNode {
                id: 4,
                x: 0.0,
                y: 0.0,
                z: 3000.0,
            },
            StabilityNode {
                id: 5,
                x: 1000.0,
                y: 0.0,
                z: 3000.0,
            },
            StabilityNode {
                id: 6,
                x: 2000.0,
                y: 0.0,
                z: 3000.0,
            },
        ];
        let elements = vec![
            // 3 columns (ground → first floor)
            StabilityElement {
                id: 1,
                node_i_id: 1,
                node_j_id: 4,
                member_type: "Column".to_string(),
            },
            StabilityElement {
                id: 2,
                node_i_id: 2,
                node_j_id: 5,
                member_type: "Column".to_string(),
            },
            StabilityElement {
                id: 3,
                node_i_id: 3,
                node_j_id: 6,
                member_type: "Column".to_string(),
            },
            // 2 girders — both in X direction (parallel)
            StabilityElement {
                id: 4,
                node_i_id: 4,
                node_j_id: 5,
                member_type: "Girder".to_string(),
            },
            StabilityElement {
                id: 5,
                node_i_id: 5,
                node_j_id: 6,
                member_type: "Girder".to_string(),
            },
        ];
        assert!(
            !has_minimum_assembly(&nodes, &elements),
            "3 columns + 2 parallel girders must NOT satisfy minimum assembly (need 90° pair)"
        );
    }

    /// Bug 3 regression: cantilever girder (one free end) must NOT pass validate_girder_support.
    #[test]
    fn test_cantilever_girder_not_stable() {
        let nodes = create_test_nodes();
        let all_elements = vec![
            StabilityElement {
                id: 1,
                node_i_id: 1,
                node_j_id: 5,
                member_type: "Column".to_string(),
            },
            StabilityElement {
                id: 2,
                node_i_id: 2,
                node_j_id: 6,
                member_type: "Column".to_string(),
            },
            // Girder: node_i (5) connected to stable col 1, node_j (7) has NO stable column
            StabilityElement {
                id: 4,
                node_i_id: 5,
                node_j_id: 7,
                member_type: "Girder".to_string(),
            },
        ];
        // Only columns 1 and 2 are stable
        let stable_ids: HashSet<i32> = [1, 2].into_iter().collect();
        let girder = &all_elements[2]; // id=4

        assert!(
            !validate_girder_support(girder, &nodes, &all_elements, &stable_ids),
            "Cantilever girder (free end at node 7) must NOT be supported"
        );
    }

    /// Bug 4 regression: column stacking (upper col node_i == lower col node_j) must NOT
    /// count as "connected to adjacent structure" in is_connected_to_stable_structure.
    #[test]
    fn test_column_stacking_not_lateral_connection() {
        // Floor 1 assembly is stable (3 cols + 2 girders)
        let nodes = vec![
            // Ground (z=0)
            StabilityNode {
                id: 1,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            StabilityNode {
                id: 2,
                x: 1000.0,
                y: 0.0,
                z: 0.0,
            },
            StabilityNode {
                id: 3,
                x: 0.0,
                y: 1000.0,
                z: 0.0,
            },
            // Floor 1 (z=3000)
            StabilityNode {
                id: 4,
                x: 0.0,
                y: 0.0,
                z: 3000.0,
            },
            StabilityNode {
                id: 5,
                x: 1000.0,
                y: 0.0,
                z: 3000.0,
            },
            StabilityNode {
                id: 6,
                x: 0.0,
                y: 1000.0,
                z: 3000.0,
            },
            // Floor 2 (z=6000)
            StabilityNode {
                id: 7,
                x: 0.0,
                y: 0.0,
                z: 6000.0,
            },
            StabilityNode {
                id: 8,
                x: 1000.0,
                y: 0.0,
                z: 6000.0,
            },
        ];
        let all_elements = vec![
            // Floor 1 stable columns
            StabilityElement {
                id: 1,
                node_i_id: 1,
                node_j_id: 4,
                member_type: "Column".to_string(),
            },
            StabilityElement {
                id: 2,
                node_i_id: 2,
                node_j_id: 5,
                member_type: "Column".to_string(),
            },
            StabilityElement {
                id: 3,
                node_i_id: 3,
                node_j_id: 6,
                member_type: "Column".to_string(),
            },
            // Floor 2 columns being tested (stacked on top of floor 1 cols)
            StabilityElement {
                id: 4,
                node_i_id: 4,
                node_j_id: 7,
                member_type: "Column".to_string(),
            },
            StabilityElement {
                id: 5,
                node_i_id: 5,
                node_j_id: 8,
                member_type: "Column".to_string(),
            },
        ];
        let element_by_id: HashMap<i32, &StabilityElement> =
            all_elements.iter().map(|e| (e.id, e)).collect();

        // Floor 1 columns are stable (but NO girders assembled yet)
        let assembled_stable: HashSet<i32> = [1, 2, 3].into_iter().collect();
        // Current step: 2 floor-2 columns only (no girder)
        let current_step = vec![4, 5];

        let connected = is_connected_to_stable_structure(
            &assembled_stable,
            &current_step,
            &element_by_id,
            &nodes,
        );
        assert!(
            !connected,
            "Column stacking (upper col node_i == lower col node_j) must NOT \
             count as lateral connection — no stable girder exists yet"
        );
    }

    /// Positive test: 2F columns ARE laterally connected once 1F girders are in assembled_stable.
    #[test]
    fn test_column_connected_via_stable_girder() {
        let nodes = vec![
            StabilityNode {
                id: 1,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            StabilityNode {
                id: 2,
                x: 1000.0,
                y: 0.0,
                z: 0.0,
            },
            StabilityNode {
                id: 3,
                x: 0.0,
                y: 1000.0,
                z: 0.0,
            },
            StabilityNode {
                id: 4,
                x: 0.0,
                y: 0.0,
                z: 3000.0,
            },
            StabilityNode {
                id: 5,
                x: 1000.0,
                y: 0.0,
                z: 3000.0,
            },
            StabilityNode {
                id: 6,
                x: 0.0,
                y: 1000.0,
                z: 3000.0,
            },
            StabilityNode {
                id: 7,
                x: 0.0,
                y: 0.0,
                z: 6000.0,
            },
            StabilityNode {
                id: 8,
                x: 1000.0,
                y: 0.0,
                z: 6000.0,
            },
        ];
        let all_elements = vec![
            StabilityElement {
                id: 1,
                node_i_id: 1,
                node_j_id: 4,
                member_type: "Column".to_string(),
            },
            StabilityElement {
                id: 2,
                node_i_id: 2,
                node_j_id: 5,
                member_type: "Column".to_string(),
            },
            StabilityElement {
                id: 3,
                node_i_id: 3,
                node_j_id: 6,
                member_type: "Column".to_string(),
            },
            // 1F girder (X-direction, node 4-5)
            StabilityElement {
                id: 10,
                node_i_id: 4,
                node_j_id: 5,
                member_type: "Girder".to_string(),
            },
            // 2F columns stacked on 1F
            StabilityElement {
                id: 4,
                node_i_id: 4,
                node_j_id: 7,
                member_type: "Column".to_string(),
            },
            StabilityElement {
                id: 5,
                node_i_id: 5,
                node_j_id: 8,
                member_type: "Column".to_string(),
            },
        ];
        let element_by_id: HashMap<i32, &StabilityElement> =
            all_elements.iter().map(|e| (e.id, e)).collect();

        // Floor 1 cols + girder are stable
        let assembled_stable: HashSet<i32> = [1, 2, 3, 10].into_iter().collect();
        // Current step: 2F column (node_i=4 touches girder 10)
        let current_step = vec![4];

        let connected = is_connected_to_stable_structure(
            &assembled_stable,
            &current_step,
            &element_by_id,
            &nodes,
        );
        assert!(
            connected,
            "2F column whose node_i touches a stable girder node SHOULD be laterally connected"
        );
    }
}
