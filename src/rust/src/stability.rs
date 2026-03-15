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
    /// Step number within workfront (1-indexed)
    pub step: i32,
    /// Element IDs in this step (2-5 elements per step based on stability)
    pub element_ids: Vec<i32>,
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
}

impl Default for TableGenerationResult {
    fn default() -> Self {
        Self {
            sequence_table: Vec::new(),
            step_table: Vec::new(),
            max_step: 0,
            workfront_count: 0,
            errors: Vec::new(),
        }
    }
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

/// Get floor level for a node based on z-coordinate
/// Returns 1-indexed floor level
fn get_floor_level(node_id: i32, nodes: &[StabilityNode]) -> Option<i32> {
    // Get all unique z values sorted
    let mut z_values: Vec<i64> = nodes
        .iter()
        .map(|n| (n.z * 1000.0).round() as i64)
        .collect();
    z_values.sort();
    z_values.dedup();

    // Get z for this node
    let node_z = get_node_coords(node_id, nodes)?.2;
    let node_z_key = (node_z * 1000.0).round() as i64;

    // Floor is 1-indexed position in sorted z values
    z_values
        .iter()
        .position(|&z| z == node_z_key)
        .map(|idx| (idx + 1) as i32)
}

/// Get floor level where a column starts (node_i bottom node)
fn get_column_floor(element: &StabilityElement, nodes: &[StabilityNode]) -> Option<i32> {
    if element.member_type != "Column" {
        return None;
    }
    get_floor_level(element.node_i_id, nodes)
}

/// Get floor column counts (columns per floor level)
fn get_floor_column_counts(
    elements: &[StabilityElement],
    nodes: &[StabilityNode],
) -> HashMap<i32, i32> {
    let mut floor_counts: HashMap<i32, i32> = HashMap::new();

    for element in elements.iter().filter(|e| e.member_type == "Column") {
        if let Some(floor) = get_column_floor(element, nodes) {
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
/// A girder's node_i and node_j must both be connected to already-stable elements.
pub fn validate_girder_support(
    element: &StabilityElement,
    _nodes: &[StabilityNode],
    all_elements: &[StabilityElement],
    stable_element_ids: &HashSet<i32>,
) -> bool {
    if element.member_type != "Girder" {
        return false;
    }

    let ni_supported = is_node_supported(element.node_i_id, all_elements, stable_element_ids);
    let nj_supported = is_node_supported(element.node_j_id, all_elements, stable_element_ids);

    ni_supported && nj_supported
}

/// Check if a node is supported by stable elements
fn is_node_supported(
    node_id: i32,
    elements: &[StabilityElement],
    stable_element_ids: &HashSet<i32>,
) -> bool {
    let connected = get_elements_at_node(node_id, elements);
    connected.iter().any(|e| stable_element_ids.contains(&e.id))
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

    // Get total columns per floor
    let total_per_floor = get_floor_column_counts(all_elements, nodes);

    // Get installed columns per floor
    let installed_elements: Vec<_> = all_elements
        .iter()
        .filter(|e| installed_element_ids.contains(&e.id))
        .cloned()
        .collect();
    let installed_per_floor = get_floor_column_counts(&installed_elements, nodes);

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
    let mut step_table: Vec<StepEntry> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    // Create element lookup by ID
    let element_by_id: HashMap<i32, &StabilityElement> =
        elements.iter().map(|e| (e.id, e)).collect();

    // Group sequence by workfront
    let mut workfront_sequences: HashMap<i32, Vec<i32>> = HashMap::new();
    for entry in sequence_table {
        workfront_sequences
            .entry(entry.workfront_id)
            .or_default()
            .push(entry.element_id);
    }

    // Process each workfront in sorted order for deterministic step assignment
    let mut sorted_workfront_ids: Vec<i32> = workfront_sequences.keys().cloned().collect();
    sorted_workfront_ids.sort();

    for workfront_id in sorted_workfront_ids {
        let workfront_element_ids = workfront_sequences
            .get(&workfront_id)
            .cloned()
            .unwrap_or_default();

        // CORRECT ALGORITHM:
        // - assembled_stable: All finalized previous steps (already stable)
        // - current_step_members: Elements in the open (current) step
        // - Close step when current_step_members form a stable configuration
        let mut assembled_stable: HashSet<i32> = HashSet::new();
        let mut current_step = 1i32;
        let mut current_step_members: Vec<i32> = Vec::new();

        for element_id in workfront_element_ids {
            if element_by_id.get(&element_id).is_none() {
                continue;
            }

            // Add element to current step (always)
            current_step_members.push(element_id);

            // Check if current step is now complete (stable)
            if step_is_complete(
                &assembled_stable,
                &current_step_members,
                &element_by_id,
                elements,
                nodes,
            ) {
                // Close the step - save it
                step_table.push(StepEntry {
                    workfront_id,
                    step: current_step,
                    element_ids: current_step_members.clone(),
                });

                // Move current step members to assembled_stable
                for id in &current_step_members {
                    assembled_stable.insert(*id);
                }

                // Start new step
                current_step_members.clear();
                current_step += 1;
            }
        }

        // Handle remaining elements: loop ended without step_is_complete() ever returning true.
        // These members could not form a stable configuration — this is a structural error.
        if !current_step_members.is_empty() {
            errors.push(format!(
                "Workfront {} step {} is incomplete: {} member(s) [{}] could not form a stable \
                 configuration. Check that all required predecessor elements are present and \
                 that the assembly satisfies minimum stability requirements.",
                workfront_id,
                current_step,
                current_step_members.len(),
                current_step_members
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            // Still record the step so the table is complete for diagnostics
            step_table.push(StepEntry {
                workfront_id,
                step: current_step,
                element_ids: current_step_members.clone(),
            });
        }
    }

    (step_table, errors)
}

/// Check if current step is complete (forms a stable configuration)
///
/// RULES:
/// 1. If current_step is DISCONNECTED from assembled_stable:
///    - Need full minimum assembly unit: 3 columns + 2 girders at 90°
/// 2. If current_step is CONNECTED to assembled_stable:
///    - Need all new members properly supported + at least one girder forming 90° connection
fn step_is_complete(
    assembled_stable: &HashSet<i32>,
    current_step_members: &[i32],
    element_by_id: &HashMap<i32, &StabilityElement>,
    all_elements: &[StabilityElement],
    nodes: &[StabilityNode],
) -> bool {
    if current_step_members.is_empty() {
        return false;
    }

    // RULE: Maximum 5 elements per step (3 columns + 2 girders = minimum assembly unit)
    // Per devplandoc.md line 188: "1개의 workfront 의 1개의 Step 에서 새로 생성되는
    // 부재의 갯수는 최소 단위 조립 앗세이의 총 부재 갯수 합계(기둥 3개, 거더 2개)를 초과할 수 없다"
    const MAX_ELEMENTS_PER_STEP: usize = 5;
    if current_step_members.len() >= MAX_ELEMENTS_PER_STEP {
        return true; // Force step completion at 5 elements
    }

    // Get current step elements
    let current_elements: Vec<StabilityElement> = current_step_members
        .iter()
        .filter_map(|id| element_by_id.get(id).map(|e| (*e).clone()))
        .collect();

    // Combined set: assembled_stable + current_step
    let mut combined_ids: HashSet<i32> = assembled_stable.clone();
    for id in current_step_members {
        combined_ids.insert(*id);
    }

    let combined_elements: Vec<StabilityElement> = combined_ids
        .iter()
        .filter_map(|id| element_by_id.get(id).map(|e| (*e).clone()))
        .collect();

    // Check if current step is connected to stable structure
    let is_connected = is_connected_to_stable_structure(
        assembled_stable,
        current_step_members,
        element_by_id,
        nodes,
    );

    if !is_connected {
        // CASE 1: Disconnected (first step or isolated workfront)
        // Need full minimum assembly: 3 columns + 2 girders at 90°
        has_minimum_assembly(nodes, &current_elements)
    } else {
        // CASE 2: Connected to existing stable structure
        // Check: all new members properly supported AND forms valid extension
        // Valid extension = at least one girder in current step connects to form 90° with existing
        all_members_supported(&current_elements, &combined_ids, all_elements, nodes)
            && has_valid_girder_connection(
                assembled_stable,
                &current_elements,
                &combined_elements,
                nodes,
            )
    }
}

/// Check if current step elements are connected to the stable structure
fn is_connected_to_stable_structure(
    assembled_stable: &HashSet<i32>,
    current_step_members: &[i32],
    element_by_id: &HashMap<i32, &StabilityElement>,
    _nodes: &[StabilityNode],
) -> bool {
    if assembled_stable.is_empty() {
        return false;
    }

    // Check if any current step element shares a node with assembled elements
    for elem_id in current_step_members {
        if let Some(elem) = element_by_id.get(elem_id) {
            for stable_id in assembled_stable {
                if let Some(stable_elem) = element_by_id.get(stable_id) {
                    // Check node connectivity
                    if elem.node_i_id == stable_elem.node_i_id
                        || elem.node_i_id == stable_elem.node_j_id
                        || elem.node_j_id == stable_elem.node_i_id
                        || elem.node_j_id == stable_elem.node_j_id
                    {
                        return true;
                    }
                }
            }
        }
    }

    false
}

/// Check if all members in current step are properly supported
fn all_members_supported(
    current_elements: &[StabilityElement],
    combined_ids: &HashSet<i32>,
    all_elements: &[StabilityElement],
    nodes: &[StabilityNode],
) -> bool {
    for elem in current_elements {
        let is_supported = if elem.member_type == "Column" {
            validate_column_support(elem, nodes, all_elements, combined_ids)
        } else {
            validate_girder_support(elem, nodes, all_elements, combined_ids)
        };

        if !is_supported {
            return false;
        }
    }
    true
}

/// Check if current step has a valid girder connection forming 90° with existing structure
///
/// For extension steps: needs at least one girder that completes a 90° configuration
fn has_valid_girder_connection(
    assembled_stable: &HashSet<i32>,
    current_elements: &[StabilityElement],
    combined_elements: &[StabilityElement],
    nodes: &[StabilityNode],
) -> bool {
    // Get girders in current step
    let current_girders: Vec<&StabilityElement> = current_elements
        .iter()
        .filter(|e| e.member_type == "Girder")
        .collect();

    if current_girders.is_empty() {
        // No girder in current step - not complete yet
        return false;
    }

    // Get girders in assembled stable
    let stable_girders: Vec<&StabilityElement> = combined_elements
        .iter()
        .filter(|e| assembled_stable.contains(&e.id) && e.member_type == "Girder")
        .collect();

    // Check if any current girder forms 90° with any stable girder
    for curr_g in &current_girders {
        let curr_dir = get_girder_direction(*curr_g, nodes);

        for stable_g in &stable_girders {
            let stable_dir = get_girder_direction(*stable_g, nodes);

            // Check perpendicular (90°)
            if are_perpendicular(&curr_dir, &stable_dir) {
                // Also need to share a column connection point
                if girders_share_column_connection(*curr_g, *stable_g, combined_elements) {
                    return true;
                }
            }
        }
    }

    // Also check: if current step forms its own 90° pair
    if current_girders.len() >= 2 {
        for i in 0..current_girders.len() {
            for j in (i + 1)..current_girders.len() {
                let dir_i = get_girder_direction(current_girders[i], nodes);
                let dir_j = get_girder_direction(current_girders[j], nodes);

                if are_perpendicular(&dir_i, &dir_j) {
                    if girders_share_column_connection(
                        current_girders[i],
                        current_girders[j],
                        combined_elements,
                    ) {
                        return true;
                    }
                }
            }
        }
    }

    false
}

/// Check if two girders share a column connection point
fn girders_share_column_connection(
    g1: &StabilityElement,
    g2: &StabilityElement,
    all_elements: &[StabilityElement],
) -> bool {
    // Get columns that g1 connects to
    let g1_nodes = [g1.node_i_id, g1.node_j_id];
    let g2_nodes = [g2.node_i_id, g2.node_j_id];

    // Find columns connected to g1
    let g1_columns: Vec<i32> = all_elements
        .iter()
        .filter(|e| {
            e.member_type == "Column"
                && (g1_nodes.contains(&e.node_i_id) || g1_nodes.contains(&e.node_j_id))
        })
        .map(|e| e.id)
        .collect();

    // Find columns connected to g2
    let g2_columns: Vec<i32> = all_elements
        .iter()
        .filter(|e| {
            e.member_type == "Column"
                && (g2_nodes.contains(&e.node_i_id) || g2_nodes.contains(&e.node_j_id))
        })
        .map(|e| e.id)
        .collect();

    // Check if they share a column
    g1_columns.iter().any(|c| g2_columns.contains(c))
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
    writeln!(file, "workfront_id,step,element_count,element_ids")?;

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
            "{},{},{},\"{}\"",
            entry.workfront_id,
            entry.step,
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

    // Get all unique floors
    let floor_counts = get_floor_column_counts(elements, nodes);
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
    let mut current_step = 0i32;

    for entry in &steps {
        // Add elements from this step
        for id in &entry.element_ids {
            installed_ids.insert(*id);
        }
        current_step = entry.step;

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
        let installed_floor_counts = get_floor_column_counts(
            &installed_elements
                .iter()
                .filter(|e| e.member_type == "Column")
                .map(|e| (*e).clone())
                .collect::<Vec<_>>(),
            nodes,
        );

        let mut row = format!(
            "{},{},{},{}",
            current_step, total_elements, total_columns, total_girders
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

/// Get floor column data for UI display
/// Returns Vec of (floor_level, total_columns) sorted by floor
pub fn get_floor_column_data(
    elements: &[StabilityElement],
    nodes: &[StabilityNode],
) -> Vec<(i32, usize)> {
    let floor_counts = get_floor_column_counts(elements, nodes);
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

    for entry in step_table {
        let step_idx = entry.step as usize;
        if step_idx < step_elements.len() {
            for elem_id in &entry.element_ids {
                if let Some(elem) = element_by_id.get(elem_id) {
                    let floor = if elem.member_type == "Column" {
                        get_column_floor(elem, nodes).unwrap_or(0)
                    } else {
                        // Girders: use z coordinate of lower node to determine floor
                        get_floor_level(elem.node_i_id, nodes).unwrap_or(0)
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
        assert!(result.unwrap_err().contains("3 columns"));
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
            ("C".to_string(), Some("B".to_string())), // Column 3 depends on B
            ("D".to_string(), Some("C".to_string())), // Girder 1 depends on C
            ("E".to_string(), Some("D".to_string())), // Girder 2 depends on D
        ];

        let result = generate_all_tables(&nodes, &elements, &element_data);

        assert!(result.errors.is_empty(), "Errors: {:?}", result.errors);
        assert_eq!(result.workfront_count, 1);
        assert_eq!(result.sequence_table.len(), 5);
        assert!(!result.step_table.is_empty());
    }

    #[test]
    fn test_step_table_for_rendering() {
        let elements = create_minimum_assembly_elements();
        let step_table = vec![StepEntry {
            workfront_id: 1,
            step: 1,
            element_ids: vec![1, 2, 3, 4, 5],
        }];

        let render_table = step_table_for_rendering(&step_table, &elements);

        assert_eq!(render_table.len(), 5);
        // All elements should be in step 1
        assert!(render_table.iter().all(|(step, _, _)| *step == 1));
    }
}
