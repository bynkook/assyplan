pub mod graphics;
pub mod sim_engine;
pub mod sim_grid;
pub mod stability;

use eframe::egui;
use pyo3::prelude::*;
use pyo3::types::PyModule;
use pyo3::wrap_pyfunction;
use crate::graphics::ui::SimScenario;
use std::fs;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

// ============================================================================
// Python-compatible data structures (pyclass)
// ============================================================================

/// Python-compatible Node representation
#[pyclass]
#[derive(Clone, Debug)]
pub struct PyNode {
    #[pyo3(get, set)]
    pub id: i32,
    #[pyo3(get, set)]
    pub x: f64,
    #[pyo3(get, set)]
    pub y: f64,
    #[pyo3(get, set)]
    pub z: f64,
}

#[pymethods]
impl PyNode {
    #[new]
    fn new(id: i32, x: f64, y: f64, z: f64) -> Self {
        Self { id, x, y, z }
    }
}

impl From<graphics::Node> for PyNode {
    fn from(node: graphics::Node) -> Self {
        Self {
            id: node.id,
            x: node.x,
            y: node.y,
            z: node.z,
        }
    }
}

/// Python-compatible Element representation
#[pyclass]
#[derive(Clone, Debug)]
pub struct PyElement {
    #[pyo3(get, set)]
    pub id: i32,
    #[pyo3(get, set)]
    pub node_i_id: i32,
    #[pyo3(get, set)]
    pub node_j_id: i32,
    #[pyo3(get, set)]
    pub member_type: String,
}

#[pymethods]
impl PyElement {
    #[new]
    fn new(id: i32, node_i_id: i32, node_j_id: i32, member_type: String) -> Self {
        Self {
            id,
            node_i_id,
            node_j_id,
            member_type,
        }
    }
}

impl From<graphics::Element> for PyElement {
    fn from(element: graphics::Element) -> Self {
        Self {
            id: element.id,
            node_i_id: element.node_i_id,
            node_j_id: element.node_j_id,
            member_type: element.member_type,
        }
    }
}

/// Python-compatible RenderData representation
#[pyclass]
#[derive(Clone, Debug)]
pub struct PyRenderData {
    #[pyo3(get, set)]
    pub nodes: Vec<PyNode>,
    #[pyo3(get, set)]
    pub elements: Vec<PyElement>,
    #[pyo3(get, set)]
    pub scale: f32,
}

#[pymethods]
impl PyRenderData {
    #[new]
    fn new() -> Self {
        Self {
            nodes: Vec::new(),
            elements: Vec::new(),
            scale: 1.0,
        }
    }

    fn add_node(&mut self, node: PyNode) {
        self.nodes.push(node);
    }

    fn add_element(&mut self, element: PyElement) {
        self.elements.push(element);
    }
}

impl Default for PyRenderData {
    fn default() -> Self {
        Self::new()
    }
}

impl From<graphics::RenderData> for PyRenderData {
    fn from(data: graphics::RenderData) -> Self {
        Self {
            nodes: data.nodes.into_iter().map(PyNode::from).collect(),
            elements: data.elements.into_iter().map(PyElement::from).collect(),
            scale: data.scale,
        }
    }
}

/// Python-compatible StepTable representation
/// Format: [(workfront_id, step, member_id), ...]
#[pyclass]
#[derive(Clone, Debug)]
pub struct PyStepTable {
    #[pyo3(get)]
    pub entries: Vec<(i32, usize, String)>, // (workfront_id, step, member_id)
    #[pyo3(get)]
    pub max_step: usize,
}

#[pymethods]
impl PyStepTable {
    #[new]
    fn new() -> Self {
        Self {
            entries: Vec::new(),
            max_step: 0,
        }
    }

    fn add_entry(&mut self, workfront_id: i32, step: usize, member_id: String) {
        self.entries.push((workfront_id, step, member_id));
        if step > self.max_step {
            self.max_step = step;
        }
    }

    fn get_entries_raw(&self) -> Vec<(i32, usize, String)> {
        self.entries.clone()
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.max_step = 0;
    }
}

impl Default for PyStepTable {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// eframe Application
// ============================================================================

#[derive(Debug)]
struct SimulationTaskResult {
    grid: sim_grid::SimGrid,
    scenarios: Vec<SimScenario>,
    best_idx: Option<usize>,
}

#[derive(Debug)]
enum SimulationTaskMessage {
    Completed(SimulationTaskResult),
    Failed(String),
}

#[derive(Debug, Clone)]
struct DevelopmentTaskResult {
    table_result: stability::TableGenerationResult,
    step_table_rendering: Vec<(i32, usize, String)>,
    step_elements: Vec<Vec<(i32, String, i32)>>,
    floor_data: Vec<(i32, usize)>,
    warnings: Vec<String>,
}

#[derive(Debug)]
enum DevelopmentTaskMessage {
    Completed(DevelopmentTaskResult),
    Failed(String),
}

pub struct AssyPlanApp {
    ui_state: graphics::UiState,
    render_data: Option<graphics::RenderData>,
    step_render_data: Option<graphics::StepRenderData>,
    view_state: graphics::ViewState,
    #[allow(dead_code)]
    file_dialog_open: bool,
    // Intermediate data for deferred construction calculation
    stability_nodes: Vec<stability::StabilityNode>,
    stability_elements: Vec<stability::StabilityElement>,
    stability_element_data: Vec<(String, Option<String>)>,
    // Simulation grid — stored after run_simulation() so View tab can render 3D
    sim_grid: Option<sim_grid::SimGrid>,
    // RenderData built once from sim_grid (cached to avoid per-frame fit recalculation)
    sim_render_data: Option<graphics::RenderData>,
    // Background simulation task communication + progress
    sim_task_rx: Option<mpsc::Receiver<SimulationTaskMessage>>,
    sim_progress_done: Option<Arc<AtomicUsize>>,
    sim_progress_total: usize,
    sim_progress_stage: Option<Arc<AtomicUsize>>,
    sim_cancel_flag: Option<Arc<AtomicBool>>,
    // Background development recalc task communication + progress
    dev_task_rx: Option<mpsc::Receiver<DevelopmentTaskMessage>>,
    dev_progress_done: Option<Arc<AtomicUsize>>,
    dev_progress_total: usize,
    dev_progress_stage: Option<Arc<AtomicUsize>>,
    dev_cancel_flag: Option<Arc<AtomicBool>>,
}

impl Default for AssyPlanApp {
    fn default() -> Self {
        Self {
            ui_state: graphics::UiState::new(),
            render_data: None,
            step_render_data: None,
            view_state: graphics::ViewState::new(),
            file_dialog_open: false,
            stability_nodes: Vec::new(),
            stability_elements: Vec::new(),
            stability_element_data: Vec::new(),
            sim_grid: None,
            sim_render_data: None,
            sim_task_rx: None,
            sim_progress_done: None,
            sim_progress_total: 0,
            sim_progress_stage: None,
            sim_cancel_flag: None,
            dev_task_rx: None,
            dev_progress_done: None,
            dev_progress_total: 0,
            dev_progress_stage: None,
            dev_cancel_flag: None,
        }
    }
}

impl AssyPlanApp {
    fn has_background_task_running(&self) -> bool {
        self.ui_state.sim_running || self.dev_task_rx.is_some()
    }

    fn run_development_calculation(
        nodes: &[stability::StabilityNode],
        elements: &[stability::StabilityElement],
        element_data: &[(String, Option<String>)],
        output_dir: Option<std::path::PathBuf>,
        progress_counter: Option<Arc<AtomicUsize>>,
        progress_stage: Option<Arc<AtomicUsize>>,
        cancel_flag: Option<Arc<AtomicBool>>,
    ) -> DevelopmentTaskResult {
        let cancelled = || {
            cancel_flag
                .as_ref()
                .map(|f| f.load(Ordering::Relaxed))
                .unwrap_or(false)
        };

        if let Some(counter) = &progress_counter {
            counter.store(0, Ordering::Relaxed);
        }
        if let Some(stage) = &progress_stage {
            stage.store(1, Ordering::Relaxed);
        }

        if cancelled() {
            return DevelopmentTaskResult {
                table_result: stability::TableGenerationResult::default(),
                step_table_rendering: Vec::new(),
                step_elements: Vec::new(),
                floor_data: Vec::new(),
                warnings: vec!["cancelled by user".to_string()],
            };
        }

        let table_result = stability::generate_all_tables(nodes, elements, element_data);
        if let Some(counter) = &progress_counter {
            counter.store(1, Ordering::Relaxed);
        }
        if let Some(stage) = &progress_stage {
            stage.store(2, Ordering::Relaxed);
        }

        if cancelled() {
            return DevelopmentTaskResult {
                table_result,
                step_table_rendering: Vec::new(),
                step_elements: Vec::new(),
                floor_data: Vec::new(),
                warnings: vec!["cancelled by user".to_string()],
            };
        }

        let step_table_rendering =
            stability::step_table_for_rendering(&table_result.step_table, elements);
        let step_elements = stability::build_step_elements_map(&table_result.step_table, elements, nodes);
        let floor_data = stability::get_floor_column_data(elements, nodes);
        if let Some(counter) = &progress_counter {
            counter.store(2, Ordering::Relaxed);
        }
        if let Some(stage) = &progress_stage {
            stage.store(3, Ordering::Relaxed);
        }

        let mut warnings: Vec<String> = Vec::new();
        if let Some(out_dir) = output_dir {
            match stability::save_development_review_tables(&table_result, elements, nodes, &out_dir)
            {
                Ok(mut notes) => warnings.append(&mut notes),
                Err(e) => warnings.push(format!("Failed to save tables: {}", e)),
            }
        }
        if let Some(counter) = &progress_counter {
            counter.store(4, Ordering::Relaxed);
        }
        if let Some(stage) = &progress_stage {
            stage.store(4, Ordering::Relaxed);
        }

        DevelopmentTaskResult {
            table_result,
            step_table_rendering,
            step_elements,
            floor_data,
            warnings,
        }
    }

    fn apply_development_result(&mut self, calc: DevelopmentTaskResult) {
        if calc
            .warnings
            .iter()
            .any(|w| w.eq_ignore_ascii_case("cancelled by user"))
        {
            self.ui_state.status_message = "Development recalc cancelled by user.".to_string();
            self.ui_state.needs_recalc = false;
            self.dev_task_rx = None;
            self.dev_progress_done = None;
            self.dev_progress_total = 0;
            self.dev_progress_stage = None;
            self.dev_cancel_flag = None;
            return;
        }

        let table_result = calc.table_result;
        let warnings = calc.warnings;

        let errors: Vec<String> = table_result.errors.clone();

        let workfront_count = table_result.workfront_count as usize;
        let max_step = table_result.max_step as usize;

        let sequence_order: Vec<usize> = if let Some(ref render_data) = self.render_data {
            let id_to_idx: std::collections::HashMap<i32, usize> = render_data
                .elements
                .iter()
                .enumerate()
                .map(|(idx, e)| (e.id, idx))
                .collect();
            table_result
                .sequence_table
                .iter()
                .filter_map(|entry| id_to_idx.get(&entry.element_id).copied())
                .collect()
        } else {
            Vec::new()
        };

        if let Some(ref render_data) = self.render_data {
            let mut step_render_data = graphics::StepRenderData::new(render_data.clone());
            step_render_data.set_step_data(calc.step_table_rendering);
            step_render_data.set_sequence_order(sequence_order);
            self.step_render_data = Some(step_render_data);
        } else {
            self.step_render_data = None;
        }

        let has_sequence_data = self
            .step_render_data
            .as_ref()
            .map(|s| !s.sequence_order.is_empty())
            .unwrap_or(false);
        let step_is_valid = !table_result.fatal && errors.is_empty() && max_step > 0;

        self.ui_state.needs_recalc = false;
        self.ui_state.validation_passed = errors.is_empty() && !table_result.fatal;
        self.ui_state.workfront_count = workfront_count;
        self.ui_state.has_sequence_data = has_sequence_data;
        self.ui_state.current_step = 1;
        self.ui_state.step_input = "1".to_string();
        self.ui_state.current_sequence = 1;

        self.ui_state.max_sequence = table_result.sequence_table.len();
        self.ui_state.has_step_data = step_is_valid;
        self.ui_state.max_step = if step_is_valid { max_step } else { 0 };

        self.ui_state.step_elements = if step_is_valid {
            calc.step_elements
        } else {
            Vec::new()
        };
        self.ui_state.floor_column_data = calc
            .floor_data
            .into_iter()
            .map(|(floor, total)| (floor, total, 0))
            .collect();

        self.update_floor_counts_for_step();

        let node_count = self.stability_nodes.len();
        let element_count = self.stability_elements.len();
        let mut status_msg = format!(
            "Calculated: {} nodes, {} elements, {} sequences, {} steps, {} workfront(s)",
            node_count,
            element_count,
            self.ui_state.max_sequence,
            self.ui_state.max_step,
            workfront_count
        );
        if table_result.fatal {
            status_msg = format!(
                "⚠ Calculation failed (fatal):\n{}\n\nSequence view is still available.",
                errors.join("\n")
            );
        } else if !errors.is_empty() {
            status_msg = format!(
                "⚠ Calculation completed with {} error(s). Step view is disabled; Sequence view is available.\n{}",
                errors.len(),
                errors.join("\n")
            );
        }
        if !warnings.is_empty() {
            status_msg.push_str(&format!(" | {} warning(s)", warnings.len()));
        }
        self.ui_state.status_message = status_msg;

        if let Some(ref mut data) = self.render_data {
            let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
            data.calculate_transform(rect, &self.view_state);
        }

        self.dev_task_rx = None;
        self.dev_progress_done = None;
        self.dev_progress_total = 0;
        self.dev_progress_stage = None;
        self.dev_cancel_flag = None;
    }

    fn poll_development_task(&mut self, ctx: &egui::Context) {
        let mut received: Option<DevelopmentTaskMessage> = None;

        if let Some(rx) = &self.dev_task_rx {
            match rx.try_recv() {
                Ok(msg) => received = Some(msg),
                Err(mpsc::TryRecvError::Empty) => {
                    let done = self
                        .dev_progress_done
                        .as_ref()
                        .map(|v| v.load(Ordering::Relaxed))
                        .unwrap_or(0);
                    let total = self.dev_progress_total.max(1);
                    let stage = self
                        .dev_progress_stage
                        .as_ref()
                        .map(|v| v.load(Ordering::Relaxed))
                        .unwrap_or(1);
                    let phase = match stage {
                        1 => "Sequence table generation",
                        2 => "Step mapping + floor extraction",
                        3 => "Output table writing",
                        4 => "Finalizing UI data",
                        _ => "Finalizing",
                    };
                    self.ui_state.status_message = format!(
                        "Development recalc running ({}/{}) | phase: {}",
                        done.min(total),
                        total,
                        phase
                    );
                    ctx.request_repaint_after(Duration::from_millis(60));
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    received = Some(DevelopmentTaskMessage::Failed(
                        "Development worker disconnected unexpectedly".to_string(),
                    ));
                }
            }
        }

        if let Some(msg) = received {
            match msg {
                DevelopmentTaskMessage::Completed(result) => self.apply_development_result(result),
                DevelopmentTaskMessage::Failed(err) => {
                    self.ui_state.needs_recalc = false;
                    self.dev_task_rx = None;
                    self.dev_progress_done = None;
                    self.dev_progress_total = 0;
                    self.dev_progress_stage = None;
                    self.dev_cancel_flag = None;
                    if err.eq_ignore_ascii_case("cancelled by user") {
                        self.ui_state.status_message =
                            "Development recalc cancelled by user.".to_string();
                    } else {
                        self.ui_state.status_message =
                            format!("Error: calculation failed: {}", err);
                    }
                }
            }
        }
    }

    /// Parse a coordinate value with error tracking
    fn parse_coordinate(
        value: Option<&str>,
        line_num: usize,
        col_name: &str,
        errors: &mut Vec<String>,
    ) -> Option<i64> {
        match value {
            Some(s) => {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    errors.push(format!("Line {}: Empty {} coordinate", line_num, col_name));
                    None
                } else {
                    match trimmed.parse::<i64>() {
                        Ok(v) => Some(v),
                        Err(_) => {
                            errors.push(format!(
                                "Line {}: Invalid {} coordinate '{}' (must be integer)",
                                line_num, col_name, trimmed
                            ));
                            None
                        }
                    }
                }
            }
            Option::None => {
                errors.push(format!(
                    "Line {}: Missing {} coordinate",
                    line_num, col_name
                ));
                None
            }
        }
    }

    /// Load CSV file and parse into render data
    ///
    /// CSV format: 구분,부재_FL,부재_CD,부재ID,node_i_x,node_i_y,node_i_z,node_j_x,node_j_y,node_j_z,선행부재_FL,선행부재_CD,선행부재ID
    /// Column indices: 4=node_i_x, 5=node_i_y, 6=node_i_z, 7=node_j_x, 8=node_j_y, 9=node_j_z
    /// Column indices: 10=선행부재_FL, 11=선행부재_CD, 12=선행부재ID (predecessor info)
    fn load_file(&mut self, path: &std::path::Path) {
        self.ui_state.status_message = format!("Loading: {}", path.display());
        self.ui_state.file_path = path.display().to_string();

        match std::fs::read_to_string(path) {
            Ok(content) => {
                // Validation error collection
                let mut errors: Vec<String> = Vec::new();
                let warnings: Vec<String> = Vec::new();

                // Step 1: Collect all unique coordinates from CSV
                let mut coords_set: std::collections::HashSet<(i64, i64, i64)> =
                    std::collections::HashSet::new();
                // element_data: (coord_i, coord_j, member_id, predecessor_id)
                let mut element_data: Vec<(
                    (i64, i64, i64),
                    (i64, i64, i64),
                    String,
                    Option<String>,
                )> = Vec::new();

                // Track member_ids for duplicate and reference validation
                let mut seen_member_ids: std::collections::HashSet<String> =
                    std::collections::HashSet::new();
                let mut workfront_count = 0usize;
                let mut line_num = 0usize;
                let mut data_line_count = 0usize;

                for line in content.lines() {
                    line_num += 1;
                    let line = line.trim();

                    // Skip empty lines, comments, and header
                    if line.is_empty() || line.starts_with('#') || line.starts_with("구분") {
                        continue;
                    }

                    data_line_count += 1;
                    let parts: Vec<&str> = line.split(',').collect();

                    // Validation: minimum column count
                    if parts.len() < 10 {
                        errors.push(format!(
                            "Line {}: Insufficient columns ({} < 10 required)",
                            line_num,
                            parts.len()
                        ));
                        continue;
                    }

                    // Parse member ID (column 3)
                    let member_id = parts
                        .get(3)
                        .map(|s| s.trim().to_string())
                        .unwrap_or_default();

                    // Validation: empty member_id
                    if member_id.is_empty() {
                        errors.push(format!("Line {}: Empty member ID", line_num));
                        continue;
                    }

                    // Validation: duplicate member_id
                    if seen_member_ids.contains(&member_id) {
                        errors.push(format!(
                            "Line {}: Duplicate member ID '{}'",
                            line_num, member_id
                        ));
                        continue;
                    }
                    seen_member_ids.insert(member_id.clone());

                    // Parse coordinates with error tracking
                    let x1 = Self::parse_coordinate(
                        parts.get(4).copied(),
                        line_num,
                        "node_i_x",
                        &mut errors,
                    );
                    let y1 = Self::parse_coordinate(
                        parts.get(5).copied(),
                        line_num,
                        "node_i_y",
                        &mut errors,
                    );
                    let z1 = Self::parse_coordinate(
                        parts.get(6).copied(),
                        line_num,
                        "node_i_z",
                        &mut errors,
                    );
                    let x2 = Self::parse_coordinate(
                        parts.get(7).copied(),
                        line_num,
                        "node_j_x",
                        &mut errors,
                    );
                    let y2 = Self::parse_coordinate(
                        parts.get(8).copied(),
                        line_num,
                        "node_j_y",
                        &mut errors,
                    );
                    let z2 = Self::parse_coordinate(
                        parts.get(9).copied(),
                        line_num,
                        "node_j_z",
                        &mut errors,
                    );

                    // Skip this element if any coordinate parsing failed
                    if x1.is_none()
                        || y1.is_none()
                        || z1.is_none()
                        || x2.is_none()
                        || y2.is_none()
                        || z2.is_none()
                    {
                        continue;
                    }

                    let x1 = x1.unwrap();
                    let y1 = y1.unwrap();
                    let z1 = z1.unwrap();
                    let x2 = x2.unwrap();
                    let y2 = y2.unwrap();
                    let z2 = z2.unwrap();

                    // Parse predecessor ID (column 12) if available
                    let predecessor_id = if parts.len() > 12 {
                        let pred = parts.get(12).map(|s| s.trim()).unwrap_or("");
                        if pred.is_empty() {
                            // Empty predecessor = Workfront starting point
                            workfront_count += 1;
                            None
                        } else {
                            Some(pred.to_string())
                        }
                    } else {
                        // No predecessor column = Workfront starting point
                        workfront_count += 1;
                        None
                    };

                    let coord_i = (x1, y1, z1);
                    let coord_j = (x2, y2, z2);

                    coords_set.insert(coord_i);
                    coords_set.insert(coord_j);
                    element_data.push((coord_i, coord_j, member_id, predecessor_id));
                }

                // Validation: empty data file
                if data_line_count == 0 {
                    self.ui_state.status_message =
                        "Error: Empty data file (no data rows found)".to_string();
                    self.ui_state.validation_passed = false;
                    return;
                }

                // Validation: no valid elements after parsing
                if element_data.is_empty() {
                    self.ui_state.status_message = format!(
                        "Error: No valid elements parsed. {} errors found",
                        errors.len()
                    );
                    self.ui_state.validation_passed = false;
                    return;
                }

                // Validation: predecessor references must exist
                for (_, _, member_id, predecessor_id) in &element_data {
                    if let Some(pred_id) = predecessor_id {
                        if !seen_member_ids.contains(pred_id) {
                            errors.push(format!(
                                "Member '{}': References non-existent predecessor '{}'",
                                member_id, pred_id
                            ));
                        }
                    }
                }

                // Validation: at least one workfront must exist
                if workfront_count == 0 {
                    errors.push(
                        "No workfront starting point found (all members have predecessors)"
                            .to_string(),
                    );
                }

                // Step 2: Sort coordinates by z → x → y and assign IDs starting from 1
                let mut sorted_coords: Vec<(i64, i64, i64)> = coords_set.into_iter().collect();
                sorted_coords.sort_by(|a, b| a.2.cmp(&b.2).then(a.0.cmp(&b.0)).then(a.1.cmp(&b.1)));

                // Step 3: Create coordinate → node_id mapping
                let coord_to_id: std::collections::HashMap<(i64, i64, i64), i32> = sorted_coords
                    .iter()
                    .enumerate()
                    .map(|(idx, coord)| (*coord, (idx + 1) as i32)) // ID starts from 1
                    .collect();

                // Step 4: Create RenderData with unique nodes
                let mut render_data = graphics::RenderData::new();

                for (idx, (x, y, z)) in sorted_coords.iter().enumerate() {
                    render_data.add_node(graphics::Node {
                        id: (idx + 1) as i32,
                        x: *x as f64,
                        y: *y as f64,
                        z: *z as f64,
                    });
                }

                // Step 5: Create elements with node ID references and build member_id to element_idx mapping
                let mut member_id_to_idx: std::collections::HashMap<String, usize> =
                    std::collections::HashMap::new();
                let mut element_predecessors: Vec<Option<String>> = Vec::new();

                for (element_idx, (coord_i, coord_j, member_id, predecessor_id)) in
                    element_data.iter().enumerate()
                {
                    let node_i_id = coord_to_id[coord_i];
                    let node_j_id = coord_to_id[coord_j];

                    // Determine member type: Column (vertical) vs Girder (horizontal)
                    let member_type = if coord_i.0 == coord_j.0 && coord_i.1 == coord_j.1 {
                        "Column".to_string() // Same x, y -> vertical member
                    } else {
                        "Girder".to_string() // Different x or y -> horizontal member
                    };

                    render_data.add_element(graphics::Element {
                        id: (element_idx + 1) as i32,
                        node_i_id,
                        node_j_id,
                        member_type,
                    });

                    member_id_to_idx.insert(member_id.clone(), element_idx);
                    element_predecessors.push(predecessor_id.clone());
                }

                // Convert render_data nodes/elements to stability module format
                // Store for deferred construction calculation
                self.stability_nodes = render_data
                    .nodes
                    .iter()
                    .map(|n| stability::StabilityNode {
                        id: n.id,
                        x: n.x,
                        y: n.y,
                        z: n.z,
                    })
                    .collect();

                self.stability_elements = render_data
                    .elements
                    .iter()
                    .map(|e| stability::StabilityElement {
                        id: e.id,
                        node_i_id: e.node_i_id,
                        node_j_id: e.node_j_id,
                        member_type: e.member_type.clone(),
                    })
                    .collect();

                // Build element_data for stability module: (member_id, predecessor_id)
                self.stability_element_data = element_data
                    .iter()
                    .map(|(_, _, member_id, pred_id)| (member_id.clone(), pred_id.clone()))
                    .collect();

                let node_count = render_data.nodes.len();
                let element_count = render_data.elements.len();

                // Handle validation results
                let error_count = errors.len();
                let warning_count = warnings.len();

                // Store render_data only if validation passed — no graphics on error
                if error_count == 0 {
                    self.render_data = Some(render_data);
                } else {
                    self.render_data = None;
                }
                self.step_render_data = None; // Will be created on Recalc

                // Update UI state
                self.ui_state.validation_passed = error_count == 0;
                self.ui_state.has_step_data = false;
                self.ui_state.has_sequence_data = false;
                self.ui_state.max_step = 0;
                self.ui_state.current_step = 1;
                self.ui_state.step_input = "1".to_string();

                if error_count > 0 {
                    // Validation failed — no data, no recalc available, show errors
                    self.ui_state.has_data = false;
                    self.ui_state.needs_recalc = false;
                    self.ui_state.status_message = format!(
                        "⚠ Load failed: {} error(s)\n{}",
                        error_count,
                        errors.join("\n")
                    );
                } else {
                    // Validation passed — data ready, prompt Recalc
                    self.ui_state.has_data = true;
                    self.ui_state.needs_recalc = true; // Highlight Recalc button

                    // Basic metrics (available immediately)
                    let column_count = self
                        .stability_elements
                        .iter()
                        .filter(|e| e.member_type == "Column")
                        .count();
                    let girder_count = self
                        .stability_elements
                        .iter()
                        .filter(|e| e.member_type == "Girder")
                        .count();
                    self.ui_state.total_elements = element_count;
                    self.ui_state.total_columns = column_count;
                    self.ui_state.total_girders = girder_count;
                    self.ui_state.workfront_count = 0; // Will be set on Recalc
                    self.ui_state.max_sequence = element_count;
                    self.ui_state.current_sequence = 1;

                    // Get floor column data (available immediately)
                    let floor_data = stability::get_floor_column_data(
                        &self.stability_elements,
                        &self.stability_nodes,
                    );
                    self.ui_state.floor_column_data = floor_data
                        .into_iter()
                        .map(|(floor, total)| (floor, total, 0))
                        .collect();

                    let mut status_msg = format!(
                        "Loaded: {} nodes, {} elements. Press Recalc to calculate construction sequence.",
                        node_count, element_count
                    );
                    if warning_count > 0 {
                        status_msg.push_str(&format!(" | {} warning(s)", warning_count));
                    }
                    self.ui_state.status_message = status_msg;
                }

                // Reset view state for zoom-to-fit on load
                self.view_state = graphics::ViewState::new();
                self.view_state.set_view_mode(graphics::ViewMode::Orbit3D);

                // Always switch to Development mode when a file is opened
                self.ui_state.display_mode = graphics::DisplayMode::Model;
                self.ui_state.mode = "Development".to_string();

                // Ensure fit is recalculated for the new data
                if let Some(ref mut data) = self.render_data {
                    data.invalidate_fit();
                }
            }
            Err(e) => {
                self.ui_state.status_message = format!("Error loading file: {}", e);
            }
        }
    }

    /// Recalculate construction sequence and generate all tables
    /// This is the heavy computation that was previously done on file load
    fn recalculate(&mut self) {
        if self.has_background_task_running() {
            self.ui_state.status_message =
                "A background calculation is already running. Please wait.".to_string();
            return;
        }

        if !self.ui_state.has_data
            && self.ui_state.display_mode != graphics::DisplayMode::Simulation
        {
            self.ui_state.status_message = "No data loaded. Please open a file first.".to_string();
            return;
        }

        // ── Simulation Mode branch ────────────────────────────────────────────
        if self.ui_state.display_mode == graphics::DisplayMode::Simulation {
            self.run_simulation();
            return;
        }

        if self.stability_elements.is_empty() {
            self.ui_state.status_message = "No data loaded. Please open a file first.".to_string();
            return;
        }

        self.ui_state.status_message = "Calculating construction sequence...".to_string();

        let nodes = self.stability_nodes.clone();
        let elements = self.stability_elements.clone();
        let element_data = self.stability_element_data.clone();
        let output_dir = if !self.ui_state.file_path.is_empty() {
            let input_path = std::path::Path::new(&self.ui_state.file_path);
            input_path.parent().map(|parent| parent.join("output"))
        } else {
            None
        };

        let (tx, rx) = mpsc::channel::<DevelopmentTaskMessage>();
        let progress = Arc::new(AtomicUsize::new(0));
        let stage = Arc::new(AtomicUsize::new(1));
        let cancel = Arc::new(AtomicBool::new(false));
        self.dev_task_rx = Some(rx);
        self.dev_progress_done = Some(progress.clone());
        self.dev_progress_total = 4;
        self.dev_progress_stage = Some(stage.clone());
        self.dev_cancel_flag = Some(cancel.clone());

        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(|| {
                AssyPlanApp::run_development_calculation(
                    &nodes,
                    &elements,
                    &element_data,
                    output_dir,
                    Some(progress),
                    Some(stage),
                    Some(cancel),
                )
            });

            match result {
                Ok(calc) => {
                    let _ = tx.send(DevelopmentTaskMessage::Completed(calc));
                }
                Err(_) => {
                    let _ = tx.send(DevelopmentTaskMessage::Failed(
                        "panic in development worker".to_string(),
                    ));
                }
            }
        });
    }

    fn apply_simulation_result(&mut self, result: SimulationTaskResult) {
        let grid = result.grid;
        let scenarios = result.scenarios;
        let best_idx = result.best_idx;

        // Store grid so the View tab can render a 3D view of installed elements
        self.sim_grid = Some(grid.clone());

        // Build RenderData once from the grid (cached — avoids per-frame fit recalculation)
        {
            let mut rd = graphics::RenderData::new();
            for n in &grid.nodes {
                rd.add_node(graphics::renderer::Node {
                    id: n.id,
                    x: n.x,
                    y: n.y,
                    z: n.z,
                });
            }
            for e in &grid.elements {
                rd.add_element(graphics::renderer::Element {
                    id: e.id,
                    node_i_id: e.node_i_id,
                    node_j_id: e.node_j_id,
                    member_type: e.member_type.clone(),
                });
            }
            self.sim_render_data = Some(rd);
        }

        let scenario_count = scenarios.len();
        let total_elements = grid.elements.len();
        let best_info = best_idx
            .and_then(|i| scenarios.get(i))
            .map(|s| {
                format!(
                    " | Best: scenario {} ({}/{} members, {} steps)",
                    s.id, s.metrics.total_members_installed, total_elements, s.metrics.total_steps
                )
            })
            .unwrap_or_default();

        self.ui_state.sim_scenarios = scenarios;
        self.ui_state.sim_selected_scenario = if self.ui_state.sim_scenarios.is_empty() {
            None
        } else {
            Some(0)
        };
        self.ui_state.sim_running = false;
        self.ui_state.sim_current_step = 1;
        self.ui_state.needs_recalc = false;
        self.sim_task_rx = None;
        self.sim_progress_done = None;
        self.sim_progress_total = 0;
        self.sim_progress_stage = None;
        self.sim_cancel_flag = None;

        self.ui_state.status_message = format!(
            "Simulation complete: {} scenarios generated{}",
            scenario_count, best_info
        );
    }

    fn poll_simulation_task(&mut self, ctx: &egui::Context) {
        let mut received: Option<SimulationTaskMessage> = None;

        if let Some(rx) = &self.sim_task_rx {
            match rx.try_recv() {
                Ok(msg) => received = Some(msg),
                Err(mpsc::TryRecvError::Empty) => {
                    if self.ui_state.sim_running {
                        let done = self
                            .sim_progress_done
                            .as_ref()
                            .map(|v| v.load(Ordering::Relaxed))
                            .unwrap_or(0);
                        let total = self.sim_progress_total.max(1);
                        let stage = self
                            .sim_progress_stage
                            .as_ref()
                            .map(|v| v.load(Ordering::Relaxed))
                            .unwrap_or(1);
                        let phase = match stage {
                            1 => "Grid pool building",
                            2 => "Scenario Monte-Carlo compute",
                            3 => "Best-scenario evaluation",
                            _ => "Finalizing",
                        };
                        self.ui_state.status_message = format!(
                            "Simulation running ({}/{} scenarios) | phase: {}",
                            done.min(total),
                            total,
                            phase
                        );
                        ctx.request_repaint_after(Duration::from_millis(60));
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    received = Some(SimulationTaskMessage::Failed(
                        "Simulation worker disconnected unexpectedly".to_string(),
                    ));
                }
            }
        }

        if let Some(msg) = received {
            match msg {
                SimulationTaskMessage::Completed(result) => {
                    self.apply_simulation_result(result);
                }
                SimulationTaskMessage::Failed(err) => {
                    self.ui_state.sim_running = false;
                    self.ui_state.needs_recalc = false;
                    self.sim_task_rx = None;
                    self.sim_progress_done = None;
                    self.sim_progress_total = 0;
                    self.sim_progress_stage = None;
                    self.sim_cancel_flag = None;
                    if err.eq_ignore_ascii_case("cancelled by user") {
                        self.ui_state.status_message =
                            "Simulation cancelled by user.".to_string();
                    } else {
                        self.ui_state.status_message = format!("Error: simulation failed: {}", err);
                    }
                }
            }
        }
    }

    /// Run the Simulation Mode: generate N scenarios with Monte-Carlo engine.
    fn run_simulation(&mut self) {
        let cfg = self.ui_state.grid_config.clone();
        let workfronts = self.ui_state.sim_workfronts.clone();
        let weights = self.ui_state.sim_weights;
        let threshold = self.ui_state.upper_floor_threshold;
        let count = self.ui_state.sim_scenario_count;

        if self.ui_state.sim_running {
            self.ui_state.status_message =
                "Simulation already running. Please wait for completion.".to_string();
            return;
        }

        // Validate: at least one workfront must be specified by user
        if workfronts.is_empty() {
            self.ui_state.status_message =
                "Error: No workfronts specified. Add at least one workfront in Settings."
                    .to_string();
            self.ui_state.needs_recalc = false;
            return;
        }

        self.ui_state.sim_running = true;
        self.ui_state.status_message = format!(
            "Running simulation: {} scenarios ({}×{} grid, {} floors)...",
            count, cfg.nx, cfg.ny, cfg.nz
        );

        let (tx, rx) = mpsc::channel::<SimulationTaskMessage>();
        let progress = Arc::new(AtomicUsize::new(0));
        let stage = Arc::new(AtomicUsize::new(1));
        let cancel = Arc::new(AtomicBool::new(false));
        self.sim_task_rx = Some(rx);
        self.sim_progress_done = Some(progress.clone());
        self.sim_progress_total = count;
        self.sim_progress_stage = Some(stage.clone());
        self.sim_cancel_flag = Some(cancel.clone());

        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(|| {
                stage.store(1, Ordering::Relaxed);
                let grid = sim_grid::SimGrid::new(cfg.nx, cfg.ny, cfg.nz, cfg.dx, cfg.dy, cfg.dz);
                stage.store(2, Ordering::Relaxed);
                let scenarios = sim_engine::run_all_scenarios_with_progress_and_cancel(
                    count,
                    &grid,
                    &workfronts,
                    weights,
                    threshold,
                    Some(progress),
                    Some(cancel.clone()),
                );
                stage.store(3, Ordering::Relaxed);
                let best_idx = scenarios
                    .iter()
                    .enumerate()
                    .max_by_key(|(_, s)| s.metrics.total_members_installed)
                    .map(|(i, _)| i);

                SimulationTaskResult {
                    grid,
                    scenarios,
                    best_idx,
                }
            });

            match result {
                Ok(task_result) => {
                    if cancel.load(Ordering::Relaxed) {
                        let _ = tx.send(SimulationTaskMessage::Failed(
                            "cancelled by user".to_string(),
                        ));
                    } else {
                        let _ = tx.send(SimulationTaskMessage::Completed(task_result));
                    }
                }
                Err(_) => {
                    let _ = tx.send(SimulationTaskMessage::Failed(
                        "panic in simulation worker".to_string(),
                    ));
                }
            }
        });
    }

    fn resolve_output_dir(&self) -> std::path::PathBuf {
        if !self.ui_state.file_path.is_empty() {
            let input_path = std::path::Path::new(&self.ui_state.file_path);
            if let Some(parent) = input_path.parent() {
                return parent.join("output");
            }
        }

        std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join("output")
    }

    /// Export simulation review files to output folder.
    /// selected_idx=None exports all scenarios, otherwise exports selected scenario only.
    fn export_simulation_debug(&self, selected_idx: Option<usize>) -> String {
        use std::collections::{HashMap, HashSet};
        use std::fs;
        use std::io::Write;

        let Some(grid) = self.sim_grid.as_ref() else {
            return "❌ No simulation grid to export. Run simulation first.".to_string();
        };

        let scenarios = &self.ui_state.sim_scenarios;
        if scenarios.is_empty() {
            return "❌ No scenarios to export. Run simulation first.".to_string();
        }

        let out_dir = self.resolve_output_dir();
        if let Err(e) = fs::create_dir_all(&out_dir) {
            return format!("❌ Failed to create output dir: {}", e);
        }

        let mut export_indices: Vec<usize> = if let Some(idx) = selected_idx {
            if idx >= scenarios.len() {
                return "❌ Selected scenario is out of range.".to_string();
            }
            vec![idx]
        } else {
            (0..scenarios.len()).collect()
        };
        export_indices.sort_unstable();

        // Base tables (once)
        let node_path = out_dir.join("sim_node_table.csv");
        let element_path = out_dir.join("sim_element_table.csv");
        if let Err(e) = stability::save_node_table(&grid.nodes, &node_path) {
            return format!("❌ Failed to write sim_node_table.csv: {}", e);
        }
        if let Err(e) = stability::save_element_table(&grid.elements, &element_path) {
            return format!("❌ Failed to write sim_element_table.csv: {}", e);
        }

        let element_by_id: HashMap<i32, &stability::StabilityElement> =
            grid.elements.iter().map(|e| (e.id, e)).collect();

        let mut errors = 0usize;

        for idx in export_indices {
            let Some(scenario) = scenarios.get(idx) else {
                continue;
            };
            let scene = scenario.id;

            let seq_path = out_dir.join(format!("sim_sequence_table_scene_{:04}.csv", scene));
            let step_path = out_dir.join(format!("sim_step_table.csv_scene_{:04}.csv", scene));
            let metric_path = out_dir.join(format!("sim_metric_table.csv_scene_{:04}.csv", scene));

            // sequence table
            let mut seq_csv = String::new();
            seq_csv.push_str("sequence_order,workfront_id,element_id,member_type,step\n");
            let mut seq_rows: Vec<(usize, i32, i32, String, usize)> = Vec::new();
            for (step_idx, step) in scenario.steps.iter().enumerate() {
                for seq in &step.sequences {
                    let wf_id = step
                        .local_steps
                        .iter()
                        .find(|ls| ls.element_ids.contains(&seq.element_id))
                        .map(|ls| ls.workfront_id)
                        .unwrap_or(step.workfront_id);
                    let member_type = element_by_id
                        .get(&seq.element_id)
                        .map(|e| e.member_type.clone())
                        .unwrap_or_else(|| "Unknown".to_string());
                    seq_rows.push((
                        seq.sequence_number,
                        wf_id,
                        seq.element_id,
                        member_type,
                        step_idx + 1,
                    ));
                }
            }
            seq_rows.sort_by_key(|row| (row.0, row.2));
            for (seq_no, wf_id, elem_id, member_type, step_no) in seq_rows {
                seq_csv.push_str(&format!(
                    "{},{},{},{},{}\n",
                    seq_no, wf_id, elem_id, member_type, step_no
                ));
            }

            if fs::File::create(&seq_path)
                .and_then(|mut f| f.write_all(seq_csv.as_bytes()))
                .is_err()
            {
                errors += 1;
            }

            // step table
            let mut step_csv = String::new();
            step_csv.push_str("workfront_id,step,pattern,floor,element_count,element_ids\n");
            for (step_idx, step) in scenario.steps.iter().enumerate() {
                let wf_ids = if step.local_steps.len() > 1 {
                    step.local_steps
                        .iter()
                        .map(|ls| ls.workfront_id.to_string())
                        .collect::<Vec<_>>()
                        .join(";")
                } else {
                    step.workfront_id.to_string()
                };
                let ids = step
                    .element_ids
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>()
                    .join(";");
                step_csv.push_str(&format!(
                    "\"{}\",{},\"{}\",{},{},\"{}\"\n",
                    wf_ids,
                    step_idx + 1,
                    step.pattern,
                    step.floor,
                    step.element_ids.len(),
                    ids
                ));
            }

            if fs::File::create(&step_path)
                .and_then(|mut f| f.write_all(step_csv.as_bytes()))
                .is_err()
            {
                errors += 1;
            }

            // metric table (cumulative)
            let mut metric_csv = String::new();
            let mut floor_totals: HashMap<i32, usize> = HashMap::new();
            for e in &grid.elements {
                if e.member_type == "Column" {
                    if let Some(floor) = grid.element_floor_by_id.get(&e.id) {
                        *floor_totals.entry(*floor).or_insert(0) += 1;
                    }
                }
            }
            let mut floors: Vec<i32> = floor_totals.keys().cloned().collect();
            floors.sort_unstable();

            metric_csv.push_str("step,cumulative_elements,cumulative_columns,cumulative_girders");
            for floor in &floors {
                metric_csv.push_str(&format!(",floor_{}_columns,floor_{}_rate", floor, floor));
            }
            metric_csv.push('\n');

            let mut installed_ids: HashSet<i32> = HashSet::new();
            for (step_idx, step) in scenario.steps.iter().enumerate() {
                for id in &step.element_ids {
                    installed_ids.insert(*id);
                }

                let mut total_columns = 0usize;
                let mut total_girders = 0usize;
                let mut installed_floor_columns: HashMap<i32, usize> = HashMap::new();
                for id in &installed_ids {
                    if let Some(elem) = element_by_id.get(id) {
                        if elem.member_type == "Column" {
                            total_columns += 1;
                            if let Some(floor) = grid.element_floor_by_id.get(id) {
                                *installed_floor_columns.entry(*floor).or_insert(0) += 1;
                            }
                        } else if elem.member_type == "Girder" {
                            total_girders += 1;
                        }
                    }
                }
                let total_elements = total_columns + total_girders;

                metric_csv.push_str(&format!(
                    "{},{},{},{}",
                    step_idx + 1,
                    total_elements,
                    total_columns,
                    total_girders
                ));
                for floor in &floors {
                    let installed = installed_floor_columns.get(floor).copied().unwrap_or(0);
                    let total = floor_totals.get(floor).copied().unwrap_or(0);
                    let rate = if total > 0 {
                        (installed as f64 / total as f64) * 100.0
                    } else {
                        0.0
                    };
                    metric_csv.push_str(&format!(",{},{:.1}", installed, rate));
                }
                metric_csv.push('\n');
            }

            if fs::File::create(&metric_path)
                .and_then(|mut f| f.write_all(metric_csv.as_bytes()))
                .is_err()
            {
                errors += 1;
            }
        }

        if errors == 0 {
            if selected_idx.is_some() {
                format!("✅ Exported selected scenario files to: {}", out_dir.display())
            } else {
                format!("✅ Exported all scenario files to: {}", out_dir.display())
            }
        } else {
            format!(
                "❌ Export completed with {} error(s). Output: {}",
                errors,
                out_dir.display()
            )
        }
    }

    /// Update floor column counts based on current_step
    /// Called when step changes to update Result tab metrics
    fn update_floor_counts_for_step(&mut self) {
        if self.ui_state.step_elements.is_empty() {
            return;
        }

        // Count columns installed up to current_step
        let mut floor_installed: std::collections::HashMap<i32, usize> =
            std::collections::HashMap::new();

        for step in 1..=self.ui_state.current_step {
            if step < self.ui_state.step_elements.len() {
                for (_, member_type, floor) in &self.ui_state.step_elements[step] {
                    if member_type == "Column" {
                        *floor_installed.entry(*floor).or_insert(0) += 1;
                    }
                }
            }
        }

        // Update floor_column_data with installed counts
        for (floor, total, installed) in &mut self.ui_state.floor_column_data {
            *installed = *floor_installed.get(floor).unwrap_or(&0);
            // Cap at total
            if *installed > *total {
                *installed = *total;
            }
        }
    }

    /// Reset all state
    fn reset(&mut self) {
        if let Some(flag) = &self.sim_cancel_flag {
            flag.store(true, Ordering::Relaxed);
        }
        if let Some(flag) = &self.dev_cancel_flag {
            flag.store(true, Ordering::Relaxed);
        }
        self.ui_state.reset();
        self.render_data = None;
        self.step_render_data = None;
        self.view_state = graphics::ViewState::new();
        // Clear intermediate stability data
        self.stability_nodes.clear();
        self.stability_elements.clear();
        self.stability_element_data.clear();
        // Clear simulation grid
        self.sim_grid = None;
        self.sim_render_data = None;
        self.sim_task_rx = None;
        self.sim_progress_done = None;
        self.sim_progress_total = 0;
        self.sim_progress_stage = None;
        self.sim_cancel_flag = None;
        self.dev_task_rx = None;
        self.dev_progress_done = None;
        self.dev_progress_total = 0;
        self.dev_progress_stage = None;
        self.dev_cancel_flag = None;
    }
}

impl eframe::App for AssyPlanApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_simulation_task(ctx);
        self.poll_development_task(ctx);

        // Top panel - Header with buttons
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("AssyPlan - Development Mode");
                ui.separator();

                // File open button
                if ui.button("📁 Open File").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("CSV/Text", &["csv", "txt"])
                        .pick_file()
                    {
                        self.load_file(&path);
                    }
                }

                // Recalc button - highlighted when calculation needed
                // Simulation mode doesn't require CSV data (generates its own grid)
                let recalc_enabled = self.ui_state.has_data
                    || self.ui_state.display_mode == graphics::DisplayMode::Simulation;
                let recalc_enabled = recalc_enabled && !self.has_background_task_running();
                let recalc_button = if self.ui_state.needs_recalc && recalc_enabled {
                    // Yellow border highlight to draw user attention
                    egui::Button::new("🔄 Recalc")
                        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(255, 200, 0)))
                } else {
                    egui::Button::new("🔄 Recalc")
                };
                if ui.add_enabled(recalc_enabled, recalc_button).clicked() {
                    self.recalculate();
                }

                // Reset button
                if ui.button("🗑 Reset").clicked() {
                    self.reset();
                }

                // Stop button (cooperative cancellation for background tasks)
                if ui
                    .add_enabled(
                        self.has_background_task_running(),
                        egui::Button::new("⏹ Stop"),
                    )
                    .clicked()
                {
                    if let Some(flag) = &self.sim_cancel_flag {
                        flag.store(true, Ordering::Relaxed);
                    }
                    if let Some(flag) = &self.dev_cancel_flag {
                        flag.store(true, Ordering::Relaxed);
                    }
                    self.ui_state.status_message =
                        "Stopping calculation... waiting for worker to exit.".to_string();
                }

                ui.separator();

                // Mode toggle
                ui.label("Mode:");
                ui.radio_value(
                    &mut self.ui_state.mode,
                    "Development".to_string(),
                    "Development",
                );
                if ui
                    .radio(self.ui_state.mode == "Simulation", "Simulation")
                    .clicked()
                {
                    self.ui_state.mode = "Simulation".to_string();
                    self.ui_state.display_mode = graphics::DisplayMode::Simulation;
                    self.ui_state.current_tab = "Settings".to_string();
                }
                // Sync mode string back when display_mode changes away from Simulation
                if self.ui_state.mode == "Simulation"
                    && self.ui_state.display_mode != graphics::DisplayMode::Simulation
                {
                    self.ui_state.mode = "Development".to_string();
                }
                if self.ui_state.mode == "Development"
                    && self.ui_state.display_mode == graphics::DisplayMode::Simulation
                {
                    self.ui_state.display_mode = graphics::DisplayMode::Model;
                }

                ui.separator();

                // Display mode toggle (Model / Construction)
                ui.label("Display:");
                if self.ui_state.display_mode == graphics::DisplayMode::Simulation {
                    // In Simulation mode: Model/Construction controls sim_view_is_model only.
                    // display_mode must stay as Simulation.
                    if ui
                        .selectable_label(self.ui_state.sim_view_is_model, "Model")
                        .clicked()
                    {
                        self.ui_state.sim_view_is_model = true;
                    }
                    if ui
                        .selectable_label(!self.ui_state.sim_view_is_model, "Construction")
                        .clicked()
                    {
                        self.ui_state.sim_view_is_model = false;
                    }
                } else {
                    // Development mode: Model/Construction change display_mode as before.
                    ui.selectable_value(
                        &mut self.ui_state.display_mode,
                        graphics::DisplayMode::Model,
                        "Model",
                    );
                    let construction_enabled = self.ui_state.has_data;
                    if ui
                        .add_enabled(
                            construction_enabled,
                            egui::SelectableLabel::new(
                                self.ui_state.display_mode == graphics::DisplayMode::Construction,
                                "Construction",
                            ),
                        )
                        .clicked()
                    {
                        self.ui_state.display_mode = graphics::DisplayMode::Construction;
                    }
                }

                ui.separator();

                // ID labels toggle — sync sub-flags when turned on
                let prev_show_ids = self.ui_state.show_id_labels;
                ui.checkbox(&mut self.ui_state.show_id_labels, "Show IDs");
                if self.ui_state.show_id_labels && !prev_show_ids {
                    // Turning on: restore both sub-flags
                    self.ui_state.show_node_ids = true;
                    self.ui_state.show_element_ids = true;
                }

                ui.separator();
                ui.label("View:");
                for mode in [
                    graphics::ViewMode::XY,
                    graphics::ViewMode::YZ,
                    graphics::ViewMode::ZX,
                    graphics::ViewMode::Orbit3D,
                ] {
                    let selected = self.view_state.view_mode == mode;
                    if ui
                        .selectable_label(selected, graphics::ViewState::mode_label(mode))
                        .clicked()
                    {
                        self.view_state.set_view_mode(mode);
                        // Invalidate fit when view mode changes (projection changes)
                        if let Some(ref mut data) = self.render_data {
                            data.invalidate_fit();
                        }
                        // Also invalidate sim render data fit (sim 3D view shares view_state)
                        if let Some(ref mut data) = self.sim_render_data {
                            data.invalidate_fit();
                        }
                        ctx.request_repaint();
                    }
                }
            });
        });

        // Construction navigation bar - only shown in Construction mode with data
        if self.ui_state.display_mode == graphics::DisplayMode::Construction
            && self.ui_state.has_data
        {
            egui::TopBottomPanel::top("construction_nav").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Dropdown menu for Sequence/Step selection
                    let old_mode = self.ui_state.construction_view_mode;
                    let has_steps = self.ui_state.has_step_data;
                    egui::ComboBox::from_id_source("construction_view_mode")
                        .selected_text(match self.ui_state.construction_view_mode {
                            graphics::ConstructionViewMode::Sequence => "Sequence",
                            graphics::ConstructionViewMode::Step => "Step",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.ui_state.construction_view_mode,
                                graphics::ConstructionViewMode::Sequence,
                                "Sequence",
                            );
                            let step_btn = ui.add_enabled(
                                has_steps,
                                egui::SelectableLabel::new(
                                    self.ui_state.construction_view_mode
                                        == graphics::ConstructionViewMode::Step,
                                    "Step",
                                ),
                            );
                            if step_btn.clicked() && has_steps {
                                self.ui_state.construction_view_mode =
                                    graphics::ConstructionViewMode::Step;
                            }
                        });

                    // Reset to 1 when mode changes
                    if self.ui_state.construction_view_mode != old_mode {
                        self.ui_state.current_step = 1;
                        self.ui_state.current_sequence = 1;
                        self.update_floor_counts_for_step();
                    }

                    ui.separator();

                    // Navigation based on selected mode
                    match self.ui_state.construction_view_mode {
                        graphics::ConstructionViewMode::Sequence => {
                            let max = self.ui_state.max_sequence;
                            if max > 0 {
                                let prev_enabled = self.ui_state.current_sequence > 1;
                                if ui
                                    .add_enabled(prev_enabled, egui::Button::new("◀"))
                                    .clicked()
                                {
                                    self.ui_state.current_sequence =
                                        self.ui_state.current_sequence.saturating_sub(1);
                                }

                                let mut slider_val = self.ui_state.current_sequence;
                                ui.spacing_mut().slider_width = 300.0;
                                if ui
                                    .add(
                                        egui::Slider::new(&mut slider_val, 1..=max)
                                            .show_value(false),
                                    )
                                    .changed()
                                {
                                    self.ui_state.current_sequence = slider_val;
                                }

                                let next_enabled = self.ui_state.current_sequence < max;
                                if ui
                                    .add_enabled(next_enabled, egui::Button::new("▶"))
                                    .clicked()
                                {
                                    self.ui_state.current_sequence =
                                        (self.ui_state.current_sequence + 1).min(max);
                                }

                                ui.label(format!("{}/{}", self.ui_state.current_sequence, max));
                            }
                        }
                        graphics::ConstructionViewMode::Step => {
                            let max = self.ui_state.max_step;
                            if max > 0 {
                                let old_step = self.ui_state.current_step;

                                let prev_enabled = self.ui_state.current_step > 1;
                                if ui
                                    .add_enabled(prev_enabled, egui::Button::new("◀"))
                                    .clicked()
                                {
                                    self.ui_state.current_step =
                                        self.ui_state.current_step.saturating_sub(1);
                                }

                                let mut slider_val = self.ui_state.current_step;
                                ui.spacing_mut().slider_width = 300.0;
                                if ui
                                    .add(
                                        egui::Slider::new(&mut slider_val, 1..=max)
                                            .show_value(false),
                                    )
                                    .changed()
                                {
                                    self.ui_state.current_step = slider_val;
                                }

                                let next_enabled = self.ui_state.current_step < max;
                                if ui
                                    .add_enabled(next_enabled, egui::Button::new("▶"))
                                    .clicked()
                                {
                                    self.ui_state.current_step =
                                        (self.ui_state.current_step + 1).min(max);
                                }

                                ui.label(format!("{}/{}", self.ui_state.current_step, max));

                                if self.ui_state.current_step != old_step {
                                    self.update_floor_counts_for_step();
                                }
                            }
                        }
                    }
                });
            });
        }

        // Left panel - Status
        egui::SidePanel::left("status_panel")
            .default_width(200.0)
            .show(ctx, |ui| {
                ui.heading("Status");
                ui.separator();

                ui.label(format!("Mode: {}", self.ui_state.mode));

                // Status message - show errors in red, warnings in yellow
                if self.ui_state.status_message.starts_with('⚠') {
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 180, 50),
                        &self.ui_state.status_message,
                    );
                } else if self.ui_state.status_message.starts_with("Error")
                    || self.ui_state.status_message.starts_with("Calc")
                        && self.ui_state.status_message.contains("failed")
                {
                    ui.colored_label(
                        egui::Color32::from_rgb(220, 80, 80),
                        &self.ui_state.status_message,
                    );
                } else {
                    ui.label(&self.ui_state.status_message);
                }

                if self.ui_state.sim_running {
                    ui.add_space(6.0);
                    let total = self.sim_progress_total.max(1);
                    let done = self
                        .sim_progress_done
                        .as_ref()
                        .map(|v| v.load(Ordering::Relaxed))
                        .unwrap_or(0)
                        .min(total);
                    let ratio = done as f32 / total as f32;
                    ui.add(
                        egui::ProgressBar::new(ratio)
                            .show_percentage()
                            .text(format!("Simulation progress: {}/{}", done, total)),
                    );
                }

                if self.dev_task_rx.is_some() {
                    ui.add_space(6.0);
                    let total = self.dev_progress_total.max(1);
                    let done = self
                        .dev_progress_done
                        .as_ref()
                        .map(|v| v.load(Ordering::Relaxed))
                        .unwrap_or(0)
                        .min(total);
                    let ratio = done as f32 / total as f32;
                    ui.add(
                        egui::ProgressBar::new(ratio)
                            .show_percentage()
                            .text(format!("Development recalc progress: {}/{}", done, total)),
                    );
                }

                if self.ui_state.has_data {
                    ui.label("Data loaded: Yes");
                    ui.label(format!(
                        "Validation: {}",
                        if self.ui_state.validation_passed {
                            "Passed"
                        } else {
                            "Not validated"
                        }
                    ));

                    if let Some(ref data) = self.render_data {
                        ui.separator();
                        ui.label(format!("Nodes: {}", data.nodes.len()));
                        ui.label(format!("Elements: {}", data.elements.len()));
                    }

                    // Show step info if available
                    if self.ui_state.has_step_data {
                        ui.separator();
                        ui.label(format!("Sequences: {}", self.ui_state.max_sequence));
                        ui.label(format!("Steps: {}", self.ui_state.max_step));
                        ui.label(format!(
                            "Display: {}",
                            match self.ui_state.display_mode {
                                graphics::DisplayMode::Model => "Model",
                                graphics::DisplayMode::Construction => "Construction",
                                graphics::DisplayMode::Simulation => "Simulation",
                            }
                        ));
                    } else if self.ui_state.max_sequence > 0 {
                        ui.separator();
                        ui.label(format!("Sequences: {}", self.ui_state.max_sequence));
                        ui.label("Steps: (Recalc needed)");
                    }
                } else {
                    ui.label("Data loaded: No");
                }

                if !self.ui_state.file_path.is_empty() {
                    ui.separator();
                    ui.label("File:");
                    ui.label(&self.ui_state.file_path);
                }
            });

        // Central panel - View with tabs
        // Handle pending export request (outside UI builder to avoid borrow conflict)
        if self.ui_state.sim_export_requested {
            self.ui_state.sim_export_requested = false;
            let selected = self.ui_state.sim_export_selected_index.take();
            let status = self.export_simulation_debug(selected);
            self.ui_state.sim_export_status = status;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            // Tabs
            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut self.ui_state.current_tab,
                    "Settings".to_string(),
                    "Settings",
                );
                ui.selectable_value(&mut self.ui_state.current_tab, "View".to_string(), "View");
                ui.selectable_value(
                    &mut self.ui_state.current_tab,
                    "Result".to_string(),
                    "Result",
                );
            });
            ui.separator();

            match self.ui_state.current_tab.as_str() {
                "Settings" => {
                    ui.heading("Settings");
                    ui.separator();

                    egui::ScrollArea::vertical()
                        .id_source("settings_tab_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {

                    ui.label("Show Model Info:");
                    ui.indent("model_info_opts", |ui| {
                        ui.checkbox(&mut self.ui_state.show_grid, "Grid");
                        ui.checkbox(&mut self.ui_state.show_nodes, "Nodes");
                        ui.checkbox(&mut self.ui_state.show_elements, "Elements");
                        // Hidden: inactive/uninstalled nodes & elements in ghost style.
                        // Only meaningful in Construction mode — disable in Model mode.
                        let is_construction = self.ui_state.display_mode != graphics::DisplayMode::Model;
                        ui.add_enabled(
                            is_construction,
                            egui::Checkbox::new(&mut self.ui_state.show_hidden, "Hidden"),
                        );
                    });

                    ui.separator();
                    // Settings 탭 Show ID Labels: prev 패턴으로 on/off 동기화
                    let prev_show_id_labels = self.ui_state.show_id_labels;
                    ui.checkbox(&mut self.ui_state.show_id_labels, "Show ID Labels");
                    if self.ui_state.show_id_labels && !prev_show_id_labels {
                        // 방금 켜진 경우 → 하위 플래그 복원
                        self.ui_state.show_node_ids = true;
                        self.ui_state.show_element_ids = true;
                    }
                    if self.ui_state.show_id_labels {
                        ui.indent("id_label_opts", |ui| {
                            ui.checkbox(&mut self.ui_state.show_node_ids, "Node ID");
                            ui.checkbox(&mut self.ui_state.show_element_ids, "Element ID");
                        });
                        // 하위 둘 다 꺼지면 부모도 off
                        if !self.ui_state.show_node_ids && !self.ui_state.show_element_ids {
                            self.ui_state.show_id_labels = false;
                        }
                    }

                    // ── Simulation Mode Settings ──────────────────────────
                    if self.ui_state.mode == "Simulation" {
                        ui.separator();
                        let grid_changed =
                            graphics::sim_ui::render_sim_settings(ui, &mut self.ui_state);
                        if grid_changed {
                            // Settings changed — highlight Recalc button to prompt re-run
                            self.ui_state.needs_recalc = true;
                        }
                    }

                    // ── Development Mode: Construction Constraints ────────
                    if self.ui_state.mode != "Simulation" {
                        ui.separator();
                        ui.heading("Construction Constraints");
                        ui.add_space(4.0);
                        ui.label("Upper-Floor Column Rate Threshold:");
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::Slider::new(
                                    &mut self.ui_state.upper_floor_threshold,
                                    0.0..=1.0,
                                )
                                .text("")
                                .fixed_decimals(2)
                                .clamp_to_range(true),
                            );
                            ui.label(format!(
                                "{:.0}%",
                                self.ui_state.upper_floor_threshold * 100.0
                            ));
                        });
                    }
                        });
                }
                "View" => {
                    // Simulation mode has its own view (grid plan), independent of render_data
                    if self.ui_state.display_mode == graphics::DisplayMode::Simulation {
                        // When a grid + selected scenario exist: split view
                        // - bottom panel: 3D render of installed elements (orbit/zoom via view_state)
                        // - remaining area: 2D grid plan with workfront click handling
                        let has_3d = self.sim_grid.is_some()
                            && (self.ui_state.sim_selected_scenario.is_some() || self.ui_state.sim_view_is_model);

                        if has_3d {
                            let total_view_height = ui.available_height();
                            let min_top_height = 120.0_f32;
                            let min_bottom_height = 100.0_f32;
                            let max_bottom_height =
                                (total_view_height - min_top_height).max(min_bottom_height);

                            // Bottom panel: 3D view (resizable, default 340px)
                            egui::TopBottomPanel::bottom("sim_3d_panel")
                                .resizable(true)
                                .default_height(340.0)
                                .min_height(min_bottom_height)
                                .max_height(max_bottom_height)
                                .show_inside(ui, |ui| {
                                    // Add a small top gap so the panel divider above is not visually glued to the nav bar.
                                    ui.add_space(4.0);

                                    // ── Step / Sequence navigation bar (Construction mode only) ─
                                    let (max_step, step_info, max_sequence, seq_info) = if !self.ui_state.sim_view_is_model {
                                        self
                                        .ui_state
                                        .sim_selected_scenario
                                        .and_then(|idx| self.ui_state.sim_scenarios.get(idx))
                                        .map(|scenario| {
                                            let ms = scenario.steps.len();
                                            let step_disp = self.ui_state.sim_current_step.min(ms).max(1);
                                            let step_info = scenario.steps.get(step_disp - 1).map(|s| {
                                                if s.local_steps.len() > 1 {
                                                    let wf_detail = s.local_steps.iter()
                                                        .map(|ls| format!("WF {}: {} ({})", ls.workfront_id, ls.element_ids.len(), ls.pattern))
                                                        .collect::<Vec<_>>()
                                                        .join(", ");
                                                    format!(
                                                        "[{}]  •  {} member(s): {:?}",
                                                        wf_detail,
                                                        s.element_ids.len(),
                                                        s.element_ids,
                                                    )
                                                } else {
                                                    format!(
                                                        "WF {}  •  Floor {}  •  {} member(s): {:?}  •  Pattern: {}",
                                                        s.workfront_id,
                                                        s.floor,
                                                        s.element_ids.len(),
                                                        s.element_ids,
                                                        s.pattern,
                                                    )
                                                }
                                            });
                                            // Compute max sequence number across all steps
                                            let max_seq: usize = scenario.steps.iter()
                                                .flat_map(|s| s.sequences.iter())
                                                .map(|seq| seq.sequence_number)
                                                .max()
                                                .unwrap_or(0);
                                            // Info for current sequence position
                                            let cur_seq = self.ui_state.sim_current_sequence;
                                            let current_seq_entries: Vec<(i32, i32, i32)> = scenario.steps.iter()
                                                .flat_map(|step| {
                                                    step.sequences.iter().filter_map(move |seq| {
                                                        if seq.sequence_number == cur_seq {
                                                            let (wf_id, floor) = step
                                                                .local_steps
                                                                .iter()
                                                                .find(|ls| ls.element_ids.contains(&seq.element_id))
                                                                .map(|ls| (ls.workfront_id, ls.floor))
                                                                .unwrap_or((step.workfront_id, step.floor));
                                                            Some((wf_id, floor, seq.element_id))
                                                        } else {
                                                            None
                                                        }
                                                    })
                                                })
                                                .collect();
                                            let seq_info = if current_seq_entries.is_empty() {
                                                None
                                            } else {
                                                let detail = current_seq_entries
                                                    .iter()
                                                    .map(|(wf_id, floor, element_id)| {
                                                        format!("WF {}: E#{} (F{})", wf_id, element_id, floor)
                                                    })
                                                    .collect::<Vec<_>>()
                                                    .join(", ");
                                                Some(format!(
                                                    "Seq {} / {}  •  {} install(s)  •  {}",
                                                    cur_seq,
                                                    max_seq,
                                                    current_seq_entries.len(),
                                                    detail
                                                ))
                                            };
                                            (ms, step_info, max_seq, seq_info)
                                        })
                                        .unwrap_or((0, None, 0, None))
                                    } else {
                                        (0, None, 0, None)
                                    };

                                    if !self.ui_state.sim_view_is_model {
                                        ui.horizontal(|ui| {
                                            // ── Scenario ComboBox ─────────────────────────────
                                            let scenario_count = self.ui_state.sim_scenarios.len();
                                            if scenario_count > 0 {
                                                if self.ui_state.sim_selected_scenario.is_none() {
                                                    self.ui_state.sim_selected_scenario = Some(0);
                                                }
                                                let current_label = self
                                                    .ui_state
                                                    .sim_selected_scenario
                                                    .map(|i| format!("Scenario {}", i + 1))
                                                    .unwrap_or_else(|| "Select...".to_string());
                                                egui::ComboBox::from_id_source("sim_scenario_select")
                                                    .selected_text(&current_label)
                                                    .width(120.0)
                                                    .show_ui(ui, |ui| {
                                                        for i in 0..scenario_count {
                                                            let label = format!("Scenario {}", i + 1);
                                                            let selected =
                                                                self.ui_state.sim_selected_scenario
                                                                    == Some(i);
                                                            if ui
                                                                .selectable_label(selected, &label)
                                                                .clicked()
                                                            {
                                                                self.ui_state.sim_selected_scenario =
                                                                    Some(i);
                                                                self.ui_state.sim_current_step = 1;
                                                                self.ui_state.sim_current_sequence = 1;
                                                                self.ui_state.sim_playing = false;
                                                            }
                                                        }
                                                    });
                                                ui.separator();
                                            }

                                            // ── Step / Sequence mode toggle ───────────────────
                                            let in_seq_mode = self.ui_state.sim_nav_sequence_mode;
                                            if ui.selectable_label(!in_seq_mode, "Step").clicked() {
                                                self.ui_state.sim_nav_sequence_mode = false;
                                            }
                                            if ui.selectable_label(in_seq_mode, "Seq").clicked() {
                                                self.ui_state.sim_nav_sequence_mode = true;
                                            }
                                            ui.separator();

                                            if !self.ui_state.sim_nav_sequence_mode {
                                                // ── Step mode ◀ slider ▶ ─────────────────────
                                                if ui
                                                    .add_enabled(
                                                        self.ui_state.sim_current_step > 1,
                                                        egui::Button::new("◀"),
                                                    )
                                                    .clicked()
                                                {
                                                    self.ui_state.sim_current_step =
                                                        self.ui_state.sim_current_step.saturating_sub(1).max(1);
                                                    self.ui_state.sim_playing = false;
                                                }
                                                if max_step > 0 {
                                                    ui.add(
                                                        egui::Slider::new(
                                                            &mut self.ui_state.sim_current_step,
                                                            1..=max_step,
                                                        )
                                                        .text("Step")
                                                        .clamp_to_range(true),
                                                    );
                                                }
                                                if ui
                                                    .add_enabled(
                                                        max_step > 0
                                                            && self.ui_state.sim_current_step < max_step,
                                                        egui::Button::new("▶"),
                                                    )
                                                    .clicked()
                                                {
                                                    self.ui_state.sim_current_step =
                                                        (self.ui_state.sim_current_step + 1).min(max_step);
                                                    self.ui_state.sim_playing = false;
                                                }
                                                ui.separator();
                                                ui.label(
                                                    egui::RichText::new(format!(
                                                        "{} / {}",
                                                        self.ui_state.sim_current_step.min(max_step.max(1)),
                                                        max_step
                                                    ))
                                                    .strong(),
                                                );
                                            } else {
                                                // ── Sequence mode ◀ slider ▶ ─────────────────
                                                if ui
                                                    .add_enabled(
                                                        self.ui_state.sim_current_sequence > 1,
                                                        egui::Button::new("◀"),
                                                    )
                                                    .clicked()
                                                {
                                                    self.ui_state.sim_current_sequence =
                                                        self.ui_state.sim_current_sequence.saturating_sub(1).max(1);
                                                }
                                                if max_sequence > 0 {
                                                    ui.add(
                                                        egui::Slider::new(
                                                            &mut self.ui_state.sim_current_sequence,
                                                            1..=max_sequence,
                                                        )
                                                        .text("Seq")
                                                        .clamp_to_range(true),
                                                    );
                                                }
                                                if ui
                                                    .add_enabled(
                                                        max_sequence > 0
                                                            && self.ui_state.sim_current_sequence < max_sequence,
                                                        egui::Button::new("▶"),
                                                    )
                                                    .clicked()
                                                {
                                                    self.ui_state.sim_current_sequence =
                                                        (self.ui_state.sim_current_sequence + 1).min(max_sequence);
                                                }
                                                ui.separator();
                                                ui.label(
                                                    egui::RichText::new(format!(
                                                        "{} / {}",
                                                        self.ui_state.sim_current_sequence.min(max_sequence.max(1)),
                                                        max_sequence
                                                    ))
                                                    .strong(),
                                                );
                                            }
                                        });

                                        // Detail line below the nav bar
                                        if !self.ui_state.sim_nav_sequence_mode {
                                            // Step detail (floor / workfront / member list / pattern)
                                            if let Some(info) = step_info {
                                                ui.label(
                                                    egui::RichText::new(info)
                                                        .size(10.0)
                                                        .color(egui::Color32::from_rgb(160, 210, 255)),
                                                );
                                            }
                                        } else {
                                            // Sequence detail (element id)
                                            if let Some(info) = seq_info {
                                                ui.label(
                                                    egui::RichText::new(info)
                                                        .size(10.0)
                                                        .color(egui::Color32::from_rgb(180, 255, 200)),
                                                );
                                            }
                                        }

                                        ui.separator();
                                    } // end Construction nav bar

                                    // ── 3D viewport (remaining space) ─────────────────────
                                    // Call handle_input unconditionally (mirrors dev mode pattern).
                                    // handle_input uses its own pointer-in-rect check for scroll/zoom.
                                    // The early-return guard in handle_input uses response.contains_pointer()
                                    // which can be unreliable inside TopBottomPanel, but orbit drag
                                    // (response.dragged_by) and zoom (pointer_in_rect check inside) both work.
                                    let rect_3d = ui.available_rect_before_wrap();
                                    let resp = ui.allocate_rect(
                                        rect_3d,
                                        egui::Sense::click_and_drag(),
                                    );
                                    if self.view_state.handle_input(&resp, ui) {
                                        ctx.request_repaint();
                                    }
                                    if let Some(ref grid) = self.sim_grid {
                                        // Render using a painter at the full 3D panel rect
                                        let painter = ui.painter_at(rect_3d);
                                        use std::collections::HashSet;

                                        // In Model mode: all elements are "installed" (fully active)
                                        // In Construction mode: elements up to current step/sequence
                                        let installed_ids: HashSet<i32> = if self.ui_state.sim_view_is_model {
                                            grid.elements.iter().map(|e| e.id).collect()
                                        } else {
                                            self
                                            .ui_state
                                            .sim_selected_scenario
                                            .and_then(|idx| {
                                                self.ui_state.sim_scenarios.get(idx)
                                            })
                                            .map(|scenario| {
                                                if !self.ui_state.sim_nav_sequence_mode {
                                                    // Step mode: show elements up to sim_current_step
                                                    let steps_to_show = self
                                                        .ui_state
                                                        .sim_current_step
                                                        .min(scenario.steps.len());
                                                    scenario.steps[..steps_to_show]
                                                        .iter()
                                                        .flat_map(|s| s.element_ids.iter().copied())
                                                        .collect()
                                                } else {
                                                    // Sequence mode: show individual members whose
                                                    // sequence_number <= sim_current_sequence
                                                    let cur_seq = self.ui_state.sim_current_sequence;
                                                    scenario.steps.iter()
                                                        .flat_map(|s| s.sequences.iter())
                                                        .filter(|seq| seq.sequence_number <= cur_seq)
                                                        .map(|seq| seq.element_id)
                                                        .collect()
                                                }
                                            })
                                            .unwrap_or_default()
                                        };

                                        painter.rect_filled(
                                            rect_3d,
                                            0.0,
                                            egui::Color32::from_gray(18),
                                        );

                                        // Use cached RenderData (built once when sim_grid was set).
                                        // Avoids per-frame fit recalculation which caused zoom
                                        // oscillation when orbit angle changed the projected bbox.
                                        if let Some(ref mut render_data) = self.sim_render_data {
                                        render_data
                                            .calculate_transform(rect_3d, &self.view_state);

                                        // Revit-style grid lines (same as dev mode)
                                        if self.ui_state.show_grid {
                                            render_data.render_grid(&painter, rect_3d, &self.view_state);
                                        }

                                        let node_map: std::collections::HashMap<
                                            i32,
                                            (f64, f64, f64),
                                        > = grid
                                            .nodes
                                            .iter()
                                            .map(|n| (n.id, (n.x, n.y, n.z)))
                                            .collect();

                                        // Ghost (uninstalled) — only when show_hidden is true
                                        if self.ui_state.show_elements && self.ui_state.show_hidden {
                                            for e in &grid.elements {
                                                if installed_ids.contains(&e.id) {
                                                    continue;
                                                }
                                                if let (
                                                    Some(&(xi, yi, zi)),
                                                    Some(&(xj, yj, zj)),
                                                ) = (
                                                    node_map.get(&e.node_i_id),
                                                    node_map.get(&e.node_j_id),
                                                ) {
                                                    let p1 = render_data.project_to_2d(
                                                        xi,
                                                        yi,
                                                        zi,
                                                        &self.view_state,
                                                    );
                                                    let p2 = render_data.project_to_2d(
                                                        xj,
                                                        yj,
                                                        zj,
                                                        &self.view_state,
                                                    );
                                                    painter.line_segment(
                                                        [p1, p2],
                                                        egui::Stroke::new(
                                                            graphics::renderer::GHOST_STROKE_WIDTH,
                                                            graphics::renderer::GHOST_COLOR,
                                                        ),
                                                    );
                                                }
                                            }
                                        }

                                        // Installed (active) — unified Dev-mode colors
                                        if self.ui_state.show_elements {
                                            let col_color = egui::Color32::from_rgb(200, 50, 50);
                                            let gdr_color = egui::Color32::from_rgb(50, 150, 50);
                                            for e in &grid.elements {
                                                if !installed_ids.contains(&e.id) {
                                                    continue;
                                                }
                                                if let (
                                                    Some(&(xi, yi, zi)),
                                                    Some(&(xj, yj, zj)),
                                                ) = (
                                                    node_map.get(&e.node_i_id),
                                                    node_map.get(&e.node_j_id),
                                                ) {
                                                    let p1 = render_data.project_to_2d(
                                                        xi,
                                                        yi,
                                                        zi,
                                                        &self.view_state,
                                                    );
                                                    let p2 = render_data.project_to_2d(
                                                        xj,
                                                        yj,
                                                        zj,
                                                        &self.view_state,
                                                    );
                                                    let color = if e.member_type == "Column" {
                                                        col_color
                                                    } else {
                                                        gdr_color
                                                    };
                                                    painter.line_segment(
                                                        [p1, p2],
                                                        egui::Stroke::new(2.0, color),
                                                    );
                                                }
                                            }
                                        }

                                        // Nodes — ghost (inactive) + active (blue dot)
                                        if self.ui_state.show_nodes {
                                            // Collect active node IDs
                                            let active_node_ids: std::collections::HashSet<i32> =
                                                grid.elements.iter()
                                                    .filter(|e| installed_ids.contains(&e.id))
                                                    .flat_map(|e| [e.node_i_id, e.node_j_id])
                                                    .collect();

                                            for n in &grid.nodes {
                                                let p = render_data.project_to_2d(
                                                    n.x,
                                                    n.y,
                                                    n.z,
                                                    &self.view_state,
                                                );
                                                if !rect_3d.contains(p) {
                                                    continue;
                                                }
                                                if active_node_ids.contains(&n.id) {
                                                    // Active: blue dot (Dev-mode standard)
                                                    painter.circle_filled(
                                                        p,
                                                        graphics::renderer::ACTIVE_NODE_RADIUS,
                                                        graphics::renderer::ACTIVE_NODE_COLOR,
                                                    );
                                                } else if self.ui_state.show_hidden {
                                                    // Inactive ghost node
                                                    painter.circle_filled(
                                                        p,
                                                        graphics::renderer::GHOST_NODE_RADIUS,
                                                        graphics::renderer::GHOST_NODE_COLOR,
                                                    );
                                                }
                                            }
                                        }

                                        // Node ID labels (only for installed/active nodes)
                                        if self.ui_state.show_id_labels
                                            && self.ui_state.show_node_ids
                                            && self.ui_state.show_nodes
                                        {
                                            let active_node_ids: std::collections::HashSet<i32> =
                                                grid.elements.iter()
                                                    .filter(|e| installed_ids.contains(&e.id))
                                                    .flat_map(|e| [e.node_i_id, e.node_j_id])
                                                    .collect();
                                            for n in &grid.nodes {
                                                if !active_node_ids.contains(&n.id) {
                                                    continue;
                                                }
                                                let p = render_data.project_to_2d(
                                                    n.x, n.y, n.z, &self.view_state,
                                                );
                                                if rect_3d.contains(p) {
                                                    painter.text(
                                                        p + egui::vec2(5.0, -5.0),
                                                        egui::Align2::LEFT_BOTTOM,
                                                        n.id.to_string(),
                                                        egui::FontId::proportional(10.0),
                                                        egui::Color32::WHITE,
                                                    );
                                                }
                                            }
                                        }

                                        // Element ID labels (only for installed/active elements)
                                        if self.ui_state.show_id_labels
                                            && self.ui_state.show_element_ids
                                            && self.ui_state.show_elements
                                        {
                                            for e in &grid.elements {
                                                if !installed_ids.contains(&e.id) {
                                                    continue;
                                                }
                                                if let (
                                                    Some(&(xi, yi, zi)),
                                                    Some(&(xj, yj, zj)),
                                                ) = (
                                                    node_map.get(&e.node_i_id),
                                                    node_map.get(&e.node_j_id),
                                                ) {
                                                    let mid = render_data.project_to_2d(
                                                        (xi + xj) / 2.0,
                                                        (yi + yj) / 2.0,
                                                        (zi + zj) / 2.0,
                                                        &self.view_state,
                                                    );
                                                    if rect_3d.contains(mid) {
                                                        painter.text(
                                                            mid + egui::vec2(3.0, -3.0),
                                                            egui::Align2::LEFT_BOTTOM,
                                                            e.id.to_string(),
                                                            egui::FontId::proportional(9.0),
                                                            egui::Color32::YELLOW,
                                                        );
                                                    }
                                                }
                                            }
                                        }

                                        // Info overlay
                                        let overlay = if self.ui_state.sim_view_is_model {
                                            format!(
                                                "Model View — {} member(s) total",
                                                installed_ids.len()
                                            )
                                        } else {
                                            let step_total = self
                                                .ui_state
                                                .sim_selected_scenario
                                                .and_then(|i| {
                                                    self.ui_state.sim_scenarios.get(i)
                                                })
                                                .map(|s| s.steps.len())
                                                .unwrap_or(0);
                                            format!(
                                                "3D View — Step {}/{} — {} member(s) installed",
                                                self.ui_state
                                                    .sim_current_step
                                                    .min(step_total.max(1)),
                                                step_total,
                                                installed_ids.len()
                                            )
                                        };
                                        painter.text(
                                            rect_3d.left_top()
                                                + egui::vec2(8.0, 8.0),
                                            egui::Align2::LEFT_TOP,
                                            overlay,
                                            egui::FontId::proportional(11.0),
                                            egui::Color32::from_rgb(120, 200, 255),
                                        );

                                        // Legend
                                        let lx = rect_3d.left() + 8.0;
                                        let ly = rect_3d.bottom() - 36.0;
                                        painter.line_segment(
                                            [
                                                egui::pos2(lx, ly + 4.0),
                                                egui::pos2(lx + 16.0, ly + 4.0),
                                            ],
                                            egui::Stroke::new(2.0, egui::Color32::from_rgb(200, 50, 50)),
                                        );
                                        painter.text(
                                            egui::pos2(lx + 18.0, ly + 4.0),
                                            egui::Align2::LEFT_CENTER,
                                            "Column",
                                            egui::FontId::proportional(9.0),
                                            egui::Color32::from_gray(180),
                                        );
                                        painter.line_segment(
                                            [
                                                egui::pos2(lx, ly + 18.0),
                                                egui::pos2(lx + 16.0, ly + 18.0),
                                            ],
                                            egui::Stroke::new(2.0, egui::Color32::from_rgb(50, 150, 50)),
                                        );
                                        painter.text(
                                            egui::pos2(lx + 18.0, ly + 18.0),
                                            egui::Align2::LEFT_CENTER,
                                            "Girder",
                                            egui::FontId::proportional(9.0),
                                            egui::Color32::from_gray(180),
                                        );
                                        } // end if let Some(ref mut render_data)
                                    }
                                    // View cube overlay — matches dev mode, using sim 3D rect
                                    graphics::axis_cube::paint_axis_cube(ctx, rect_3d, &self.view_state);
                                });
                        }

                        // 2D grid plan (remaining area, handles workfront clicks)
                        if graphics::sim_ui::render_sim_view(ui, &mut self.ui_state) {
                            self.ui_state.needs_recalc = true;
                            ctx.request_repaint();
                        }
                    } else {
                    // Render viewport based on display mode
                    let has_render_data = self.render_data.is_some();
                    #[allow(unused_variables)]
                    let has_step_data = self.step_render_data.is_some()
                        && self
                            .step_render_data
                            .as_ref()
                            .map_or(false, |s| s.has_steps());

                    if has_render_data {
                        let rect = ui.available_rect_before_wrap();
                        let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
                        if self.view_state.handle_input(&response, ui) {
                            ctx.request_repaint();
                        }

                        let painter = ui.painter_at(rect);
                        painter.rect_filled(rect, 0.0, egui::Color32::from_gray(18));

                        let visibility = graphics::VisibilitySettings {
                            show_grid: self.ui_state.show_grid,
                            show_nodes: self.ui_state.show_nodes,
                            show_elements: self.ui_state.show_elements,
                            show_hidden: self.ui_state.show_hidden,
                        };

                        // Render based on display mode
                        match self.ui_state.display_mode {
                            graphics::DisplayMode::Model => {
                                // Standard Model view - show all elements
                                if let Some(ref mut data) = self.render_data {
                                    data.calculate_transform(rect, &self.view_state);
                                    data.render(&painter, rect, &self.view_state, &visibility);

                                    // Draw node IDs if enabled
                                    if self.ui_state.show_id_labels
                                        && self.ui_state.show_node_ids
                                        && self.ui_state.show_nodes
                                    {
                                        for node in &data.nodes {
                                            let pos = data.project_to_2d(
                                                node.x,
                                                node.y,
                                                node.z,
                                                &self.view_state,
                                            );
                                            if rect.contains(pos) {
                                                painter.text(
                                                    pos + egui::vec2(5.0, -5.0),
                                                    egui::Align2::LEFT_BOTTOM,
                                                    node.id.to_string(),
                                                    egui::FontId::proportional(10.0),
                                                    egui::Color32::WHITE,
                                                );
                                            }
                                        }
                                    }

                                    // Draw element IDs at midpoint if enabled
                                    if self.ui_state.show_id_labels
                                        && self.ui_state.show_element_ids
                                        && self.ui_state.show_elements
                                    {
                                        for element in &data.elements {
                                            let node_i = data
                                                .nodes
                                                .iter()
                                                .find(|n| n.id == element.node_i_id);
                                            let node_j = data
                                                .nodes
                                                .iter()
                                                .find(|n| n.id == element.node_j_id);

                                            if let (Some(ni), Some(nj)) = (node_i, node_j) {
                                                let mid_x = (ni.x + nj.x) / 2.0;
                                                let mid_y = (ni.y + nj.y) / 2.0;
                                                let mid_z = (ni.z + nj.z) / 2.0;

                                                let pos = data.project_to_2d(
                                                    mid_x,
                                                    mid_y,
                                                    mid_z,
                                                    &self.view_state,
                                                );
                                                if rect.contains(pos) {
                                                    painter.text(
                                                        pos + egui::vec2(3.0, -3.0),
                                                        egui::Align2::LEFT_BOTTOM,
                                                        element.id.to_string(),
                                                        egui::FontId::proportional(9.0),
                                                        egui::Color32::YELLOW,
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            graphics::DisplayMode::Construction => {
                                // Construction view: sequence rendering is independent from step validity.
                                if !self.ui_state.has_step_data && !self.ui_state.has_sequence_data {
                                    let msg = if self.ui_state.needs_recalc {
                                        "Press Recalc to calculate construction sequence.".to_string()
                                    } else if !self.ui_state.validation_passed {
                                        "⚠ Construction sequence has errors.\nCheck Status or Result tab.".to_string()
                                    } else {
                                        "No construction data.".to_string()
                                    };
                                    painter.text(
                                        rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        msg,
                                        egui::FontId::proportional(16.0),
                                        egui::Color32::from_rgb(200, 200, 100),
                                    );
                                } else {
                                match self.ui_state.construction_view_mode {
                                    graphics::ConstructionViewMode::Sequence => {
                                        if !self.ui_state.has_sequence_data {
                                            painter.text(
                                                rect.center(),
                                                egui::Align2::CENTER_CENTER,
                                                "No sequence data available.",
                                                egui::FontId::proportional(16.0),
                                                egui::Color32::from_rgb(200, 200, 100),
                                            );
                                        } else {
                                            // Sequence mode: render elements 1 to current_sequence
                                            // Uses step_render_data.base for transform (same as Step mode)
                                            if let Some(ref mut data) = self.render_data {
                                            data.calculate_transform(rect, &self.view_state);
                                            let mut shown_element_ids =
                                                std::collections::HashSet::new();
                                            let mut shown_node_ids =
                                                std::collections::HashSet::new();

                                            // Use step_render_data for rendering with sequence mode
                                            if let Some(ref mut step_data) = self.step_render_data {
                                                // Must calculate transform on step_data.base so that
                                                // render_sequence uses up-to-date zoom/pan from view_state
                                                step_data
                                                    .base
                                                    .calculate_transform(rect, &self.view_state);

                                                let total = if !step_data.sequence_order.is_empty() {
                                                    step_data.sequence_order.len()
                                                } else {
                                                    step_data.base.elements.len()
                                                };
                                                let max_pos = self
                                                    .ui_state
                                                    .current_sequence
                                                    .min(total);
                                                for pos in 0..max_pos {
                                                    let elem_idx = if !step_data.sequence_order.is_empty() {
                                                        step_data.sequence_order[pos]
                                                    } else {
                                                        pos
                                                    };
                                                    if let Some(element) =
                                                        step_data.base.elements.get(elem_idx)
                                                    {
                                                        shown_element_ids.insert(element.id);
                                                        shown_node_ids.insert(element.node_i_id);
                                                        shown_node_ids.insert(element.node_j_id);
                                                    }
                                                }

                                                step_data.render_sequence(
                                                    &painter,
                                                    rect,
                                                    &self.view_state,
                                                    &visibility,
                                                    self.ui_state.current_sequence,
                                                );
                                            } else {
                                                // Fallback: render all elements if no step_data
                                                data.render(
                                                    &painter,
                                                    rect,
                                                    &self.view_state,
                                                    &visibility,
                                                );
                                            }

                                            // Draw sequence info overlay
                                            let seq_info = format!(
                                                "Sequence {}/{}",
                                                self.ui_state.current_sequence,
                                                self.ui_state.max_sequence
                                            );
                                            painter.text(
                                                rect.left_top() + egui::vec2(10.0, 10.0),
                                                egui::Align2::LEFT_TOP,
                                                seq_info,
                                                egui::FontId::proportional(14.0),
                                                egui::Color32::from_rgb(100, 200, 255),
                                            );

                                            // Draw node IDs if enabled
                                            if self.ui_state.show_id_labels
                                                && self.ui_state.show_node_ids
                                                && self.ui_state.show_nodes
                                            {
                                                for node in &data.nodes {
                                                    if !shown_node_ids.contains(&node.id) {
                                                        continue;
                                                    }
                                                    let pos = data.project_to_2d(
                                                        node.x,
                                                        node.y,
                                                        node.z,
                                                        &self.view_state,
                                                    );
                                                    if rect.contains(pos) {
                                                        painter.text(
                                                            pos + egui::vec2(5.0, -5.0),
                                                            egui::Align2::LEFT_BOTTOM,
                                                            node.id.to_string(),
                                                            egui::FontId::proportional(10.0),
                                                            egui::Color32::WHITE,
                                                        );
                                                    }
                                                }
                                            }

                                            // Draw element IDs at midpoint if enabled
                                            if self.ui_state.show_id_labels
                                                && self.ui_state.show_element_ids
                                                && self.ui_state.show_elements
                                            {
                                                for element in &data.elements {
                                                    if !shown_element_ids.contains(&element.id) {
                                                        continue;
                                                    }
                                                    let node_i = data
                                                        .nodes
                                                        .iter()
                                                        .find(|n| n.id == element.node_i_id);
                                                    let node_j = data
                                                        .nodes
                                                        .iter()
                                                        .find(|n| n.id == element.node_j_id);
                                                    if let (Some(ni), Some(nj)) = (node_i, node_j) {
                                                        let mid_x = (ni.x + nj.x) / 2.0;
                                                        let mid_y = (ni.y + nj.y) / 2.0;
                                                        let mid_z = (ni.z + nj.z) / 2.0;
                                                        let pos = data.project_to_2d(
                                                            mid_x,
                                                            mid_y,
                                                            mid_z,
                                                            &self.view_state,
                                                        );
                                                        if rect.contains(pos) {
                                                            painter.text(
                                                                pos + egui::vec2(3.0, -3.0),
                                                                egui::Align2::LEFT_BOTTOM,
                                                                element.id.to_string(),
                                                                egui::FontId::proportional(9.0),
                                                                egui::Color32::YELLOW,
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                            }
                                        }
                                    }
                                    graphics::ConstructionViewMode::Step => {
                                        if !self.ui_state.has_step_data {
                                            painter.text(
                                                rect.center(),
                                                egui::Align2::CENTER_CENTER,
                                                "⚠ Step generation failed.\nSwitch to Sequence mode to inspect input order.",
                                                egui::FontId::proportional(16.0),
                                                egui::Color32::from_rgb(255, 180, 50),
                                            );
                                        } else if let Some(ref mut step_data) = self.step_render_data {
                                            // Sync current step from UI state to step_render_data
                                            step_data.set_current_step(self.ui_state.current_step);
                                            let cumulative_indices =
                                                step_data.get_cumulative_elements(
                                                    self.ui_state.current_step,
                                                );
                                            let mut shown_element_ids =
                                                std::collections::HashSet::new();
                                            let mut shown_node_ids =
                                                std::collections::HashSet::new();
                                            for idx in cumulative_indices {
                                                if let Some(element) =
                                                    step_data.base.elements.get(idx)
                                                {
                                                    shown_element_ids.insert(element.id);
                                                    shown_node_ids.insert(element.node_i_id);
                                                    shown_node_ids.insert(element.node_j_id);
                                                }
                                            }

                                            // Calculate transform using base render data
                                            step_data
                                                .base
                                                .calculate_transform(rect, &self.view_state);

                                            // Render with step-based coloring
                                            step_data.render_step(
                                                &painter,
                                                rect,
                                                &self.view_state,
                                                &visibility,
                                            );

                                            // Draw step info overlay
                                            let step_info = format!(
                                                "Step {}/{}",
                                                self.ui_state.current_step, self.ui_state.max_step
                                            );
                                            painter.text(
                                                rect.left_top() + egui::vec2(10.0, 10.0),
                                                egui::Align2::LEFT_TOP,
                                                step_info,
                                                egui::FontId::proportional(14.0),
                                                egui::Color32::from_rgb(255, 200, 100),
                                            );

                                            // Draw node IDs if enabled
                                            if self.ui_state.show_id_labels
                                                && self.ui_state.show_node_ids
                                                && self.ui_state.show_nodes
                                            {
                                                for node in &step_data.base.nodes {
                                                    if !shown_node_ids.contains(&node.id) {
                                                        continue;
                                                    }
                                                    let pos = step_data.base.project_to_2d(
                                                        node.x,
                                                        node.y,
                                                        node.z,
                                                        &self.view_state,
                                                    );
                                                    if rect.contains(pos) {
                                                        painter.text(
                                                            pos + egui::vec2(5.0, -5.0),
                                                            egui::Align2::LEFT_BOTTOM,
                                                            node.id.to_string(),
                                                            egui::FontId::proportional(10.0),
                                                            egui::Color32::WHITE,
                                                        );
                                                    }
                                                }
                                            }

                                            // Draw element IDs at midpoint if enabled
                                            if self.ui_state.show_id_labels
                                                && self.ui_state.show_element_ids
                                                && self.ui_state.show_elements
                                            {
                                                for element in &step_data.base.elements {
                                                    if !shown_element_ids.contains(&element.id) {
                                                        continue;
                                                    }
                                                    let node_i = step_data
                                                        .base
                                                        .nodes
                                                        .iter()
                                                        .find(|n| n.id == element.node_i_id);
                                                    let node_j = step_data
                                                        .base
                                                        .nodes
                                                        .iter()
                                                        .find(|n| n.id == element.node_j_id);
                                                    if let (Some(ni), Some(nj)) = (node_i, node_j) {
                                                        let mid_x = (ni.x + nj.x) / 2.0;
                                                        let mid_y = (ni.y + nj.y) / 2.0;
                                                        let mid_z = (ni.z + nj.z) / 2.0;
                                                        let pos = step_data.base.project_to_2d(
                                                            mid_x,
                                                            mid_y,
                                                            mid_z,
                                                            &self.view_state,
                                                        );
                                                        if rect.contains(pos) {
                                                            painter.text(
                                                                pos + egui::vec2(3.0, -3.0),
                                                                egui::Align2::LEFT_BOTTOM,
                                                                element.id.to_string(),
                                                                egui::FontId::proportional(9.0),
                                                                egui::Color32::YELLOW,
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                } // end else has_step_data
                            }
                            graphics::DisplayMode::Simulation => {
                                // Simulation mode view — grid plan + workfront selector
                                if graphics::sim_ui::render_sim_view(ui, &mut self.ui_state) {
                                    self.ui_state.needs_recalc = true;
                                    ctx.request_repaint();
                                }
                            }
                        }

                        // Always draw axis cube on its own Foreground layer
                        graphics::axis_cube::paint_axis_cube(ctx, rect, &self.view_state);
                    } else {
                        ui.centered_and_justified(|ui| {
                            ui.label("No data loaded.\nClick 'Open File' to load a CSV file.");
                        });
                    }
                    } // end else (non-Simulation display_mode)
                }
                "Result" => {
                    if self.ui_state.display_mode == graphics::DisplayMode::Simulation {
                        egui::ScrollArea::vertical()
                            .id_source("sim_result_tab_scroll")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                graphics::sim_ui::render_sim_result(ui, &mut self.ui_state);
                            });
                    } else {
                        // Use full Result tab UI with metrics, progress bars, floor data
                        graphics::render_result_tab(ui, &self.ui_state);
                    }
                }
                _ => {}
            }
        });
    }
}

pub fn run() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_title("AssyPlan - Development Mode"),
        ..Default::default()
    };
    eframe::run_native(
        "AssyPlan",
        options,
        Box::new(|_cc| Box::new(AssyPlanApp::default()) as Box<dyn eframe::App>),
    )
}

// ============================================================================
// PyO3 Functions and Module
// ============================================================================

/// Load and validate a CSV file, returning RenderData
#[pyfunction]
fn load_and_validate(path: &str) -> PyResult<PyRenderData> {
    let content = fs::read_to_string(path)
        .map_err(|e| pyo3::exceptions::PyFileNotFoundError::new_err(e.to_string()))?;

    let mut render_data = PyRenderData::new();
    let mut node_id_counter = 1i32;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() >= 4 {
            let member_type = parts[0].trim();
            let x1: f64 = parts[1].trim().parse().unwrap_or(0.0);
            let y1: f64 = parts[2].trim().parse().unwrap_or(0.0);
            let z1: f64 = parts[3].trim().parse().unwrap_or(0.0);

            let node_i = PyNode::new(node_id_counter, x1, y1, z1);
            render_data.add_node(node_i);

            if parts.len() >= 7 {
                let x2: f64 = parts[4].trim().parse().unwrap_or(0.0);
                let y2: f64 = parts[5].trim().parse().unwrap_or(0.0);
                let z2: f64 = parts[6].trim().parse().unwrap_or(0.0);

                let node_j = PyNode::new(node_id_counter + 1, x2, y2, z2);
                render_data.add_node(node_j);

                let element = PyElement::new(
                    render_data.elements.len() as i32 + 1,
                    node_id_counter,
                    node_id_counter + 1,
                    member_type.to_string(),
                );
                render_data.add_element(element);

                node_id_counter += 2;
            } else {
                node_id_counter += 1;
            }
        }
    }

    Ok(render_data)
}

/// Render data using eframe (releases GIL during rendering)
#[pyfunction]
fn render_data(data: &PyRenderData) -> PyResult<()> {
    // Convert Python data to Rust RenderData
    let mut rust_data = graphics::RenderData::new();
    rust_data.scale = data.scale;

    for py_node in &data.nodes {
        rust_data.add_node(graphics::Node {
            id: py_node.id,
            x: py_node.x,
            y: py_node.y,
            z: py_node.z,
        });
    }

    for py_element in &data.elements {
        rust_data.add_element(graphics::Element {
            id: py_element.id,
            node_i_id: py_element.node_i_id,
            node_j_id: py_element.node_j_id,
            member_type: py_element.member_type.clone(),
        });
    }

    // Launch eframe app with the data
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_title("AssyPlan - Render View"),
        ..Default::default()
    };

    // Pass data to app via heap allocation
    let data_ptr = Box::into_raw(Box::new(rust_data));
    eframe::run_native(
        "AssyPlan",
        options,
        Box::new(move |_cc| {
            let rd = unsafe { Box::from_raw(data_ptr) };
            Box::new(AssyPlanApp {
                ui_state: graphics::UiState::new(),
                render_data: Some(*rd),
                step_render_data: None,
                view_state: graphics::ViewState::new(),
                file_dialog_open: false,
                stability_nodes: Vec::new(),
                stability_elements: Vec::new(),
                stability_element_data: Vec::new(),
                sim_grid: None,
                sim_render_data: None,
                sim_task_rx: None,
                sim_progress_done: None,
                sim_progress_total: 0,
                sim_progress_stage: None,
                sim_cancel_flag: None,
                dev_task_rx: None,
                dev_progress_done: None,
                dev_progress_total: 0,
                dev_progress_stage: None,
                dev_cancel_flag: None,
            }) as Box<dyn eframe::App>
        }),
    )
    .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
}

/// Load step data from a PyStepTable
#[pyfunction]
fn load_step_data(step_table: &PyStepTable) -> PyResult<usize> {
    // Returns max_step for verification
    Ok(step_table.max_step)
}

/// Get step table info
#[pyfunction]
fn get_step_table_info(step_table: &PyStepTable) -> PyResult<String> {
    Ok(format!(
        "StepTable with {} entries, max_step={}",
        step_table.entries.len(),
        step_table.max_step
    ))
}

/// Create a PyStepTable from Python list of tuples
#[pyfunction]
fn create_step_table(entries: Vec<(i32, usize, String)>) -> PyResult<PyStepTable> {
    let mut table = PyStepTable::new();
    for (workfront_id, step, member_id) in entries {
        table.add_entry(workfront_id, step, member_id);
    }
    Ok(table)
}

#[pymodule]
fn assyplan(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(load_and_validate, m)?)?;
    m.add_function(wrap_pyfunction!(render_data, m)?)?;
    m.add_function(wrap_pyfunction!(load_step_data, m)?)?;
    m.add_function(wrap_pyfunction!(get_step_table_info, m)?)?;
    m.add_function(wrap_pyfunction!(create_step_table, m)?)?;
    m.add_class::<PyNode>()?;
    m.add_class::<PyElement>()?;
    m.add_class::<PyRenderData>()?;
    m.add_class::<PyStepTable>()?;
    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use crate::sim_grid::SimGrid;

    use super::AssyPlanApp;

    #[test]
    fn test_app_struct_exists() {
        assert!(true);
    }

    #[test]
    fn test_pystep_table_new() {
        use super::PyStepTable;

        let table = PyStepTable::new();
        assert!(table.entries.is_empty());
        assert_eq!(table.max_step, 0);
    }

    #[test]
    fn test_pystep_table_add_entry() {
        use super::PyStepTable;

        let mut table = PyStepTable::new();
        table.add_entry(1, 1, "member_1".to_string());

        assert_eq!(table.entries.len(), 1);
        assert_eq!(table.max_step, 1);
        assert_eq!(table.entries[0], (1, 1, "member_1".to_string()));
    }

    #[test]
    fn test_pystep_table_multiple_entries() {
        use super::PyStepTable;

        let mut table = PyStepTable::new();
        table.add_entry(1, 1, "member_1".to_string());
        table.add_entry(2, 1, "member_2".to_string());
        table.add_entry(3, 2, "member_3".to_string());
        table.add_entry(4, 2, "member_4".to_string());

        assert_eq!(table.entries.len(), 4);
        assert_eq!(table.max_step, 2);
    }

    #[test]
    fn test_pystep_table_get_entries() {
        use super::PyStepTable;

        let mut table = PyStepTable::new();
        table.add_entry(1, 1, "member_1".to_string());

        let entries = table.get_entries_raw();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn test_pystep_table_clear() {
        use super::PyStepTable;

        let mut table = PyStepTable::new();
        table.add_entry(1, 1, "member_1".to_string());
        table.clear();

        assert!(table.entries.is_empty());
        assert_eq!(table.max_step, 0);
    }

    #[test]
    fn test_pystep_table_order_preserved() {
        use super::PyStepTable;

        let mut table = PyStepTable::new();
        table.add_entry(3, 2, "member_c".to_string());
        table.add_entry(1, 1, "member_a".to_string());
        table.add_entry(2, 1, "member_b".to_string());

        assert_eq!(table.entries[0].0, 3);
        assert_eq!(table.entries[1].0, 1);
        assert_eq!(table.entries[2].0, 2);
    }

    #[test]
    fn test_ab_development_calc_with_progress_matches_without_progress() {
        let grid = SimGrid::new(3, 3, 3, 6000.0, 6000.0, 4000.0);
        let nodes = grid.nodes.clone();
        let elements = grid.elements.clone();

        let mut element_data = Vec::new();
        for (idx, element) in elements.iter().enumerate() {
            let member_id = format!("E{}", element.id);
            let predecessor = if idx == 0 {
                None
            } else {
                Some(format!("E{}", elements[idx - 1].id))
            };
            element_data.push((member_id, predecessor));
        }

        let no_progress = AssyPlanApp::run_development_calculation(
            &nodes,
            &elements,
            &element_data,
            None,
            None,
            None,
            None,
        );

        let progress = Arc::new(AtomicUsize::new(0));
        let with_progress = AssyPlanApp::run_development_calculation(
            &nodes,
            &elements,
            &element_data,
            None,
            Some(progress.clone()),
            None,
            None,
        );

        assert_eq!(progress.load(Ordering::Relaxed), 4);

        assert_eq!(
            no_progress.table_result.sequence_table.len(),
            with_progress.table_result.sequence_table.len()
        );
        assert_eq!(
            no_progress.table_result.step_table.len(),
            with_progress.table_result.step_table.len()
        );
        assert_eq!(no_progress.table_result.max_step, with_progress.table_result.max_step);
        assert_eq!(
            no_progress.step_table_rendering.len(),
            with_progress.step_table_rendering.len()
        );
        assert_eq!(no_progress.step_elements, with_progress.step_elements);
        assert_eq!(no_progress.floor_data, with_progress.floor_data);
    }
}
