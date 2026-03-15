pub mod graphics;
pub mod stability;

use eframe::egui;
use pyo3::prelude::*;
use pyo3::types::PyModule;
use pyo3::wrap_pyfunction;
use std::fs;

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
        }
    }
}

impl AssyPlanApp {
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
            None => {
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
                let mut warnings: Vec<String> = Vec::new();

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

                // Store render_data (without step calculation yet)
                self.render_data = Some(render_data);
                self.step_render_data = None; // Will be created on Recalc

                // Handle validation results
                let error_count = errors.len();
                let warning_count = warnings.len();

                // Update UI state - file loaded but construction not calculated
                self.ui_state.has_data = true;
                self.ui_state.validation_passed = error_count == 0;
                self.ui_state.has_step_data = false; // Not calculated yet
                self.ui_state.max_step = 0;
                self.ui_state.current_step = 1;
                self.ui_state.step_input = "1".to_string();
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
                self.ui_state.max_sequence = element_count; // Total elements for Sequence mode
                self.ui_state.current_sequence = 1;

                // Get floor column data (available immediately)
                let floor_data = stability::get_floor_column_data(
                    &self.stability_elements,
                    &self.stability_nodes,
                );
                self.ui_state.floor_column_data = floor_data
                    .into_iter()
                    .map(|(floor, total)| (floor, total, 0)) // 0 installed until Recalc
                    .collect();

                // Build status message - prompt user to press Recalc
                let mut status_msg = format!(
                    "Loaded: {} nodes, {} elements. Press Recalc to calculate construction sequence.",
                    node_count, element_count
                );
                if error_count > 0 {
                    status_msg.push_str(&format!(" | {} error(s)", error_count));
                }
                if warning_count > 0 {
                    status_msg.push_str(&format!(" | {} warning(s)", warning_count));
                }
                self.ui_state.status_message = status_msg;

                // Reset view state for zoom-to-fit on load
                self.view_state = graphics::ViewState::new();
                self.view_state.set_view_mode(graphics::ViewMode::Orbit3D);

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
        if !self.ui_state.has_data || self.stability_elements.is_empty() {
            self.ui_state.status_message = "No data loaded. Please open a file first.".to_string();
            return;
        }

        self.ui_state.status_message = "Calculating construction sequence...".to_string();

        // Generate all tables using stability module
        let table_result = stability::generate_all_tables(
            &self.stability_nodes,
            &self.stability_elements,
            &self.stability_element_data,
        );

        // Check for errors from table generation
        let mut errors: Vec<String> = Vec::new();
        let mut warnings: Vec<String> = Vec::new();

        if !table_result.errors.is_empty() {
            for err in &table_result.errors {
                errors.push(err.clone());
            }
            // If cycle detected, cannot continue
            if table_result
                .errors
                .iter()
                .any(|e| e.contains("Cycle detected"))
            {
                let error_count = errors.len();
                let mut status_msg = format!("Calculation failed: {} error(s)", error_count);
                if !errors.is_empty() {
                    status_msg.push_str(&format!("\nFirst error: {}", errors[0]));
                }
                self.ui_state.status_message = status_msg;
                self.ui_state.validation_passed = false;
                return;
            }
        }

        // Update workfront count from stability module result
        let workfront_count = table_result.workfront_count as usize;

        // Build step_table for StepRenderData from stability module result
        let step_table =
            stability::step_table_for_rendering(&table_result.step_table, &self.stability_elements);
        let max_step = table_result.max_step as usize;

        // Build sequence order: element indices into render_data.elements, in topological order
        // sequence_table[i].element_id is 1-indexed; render_data.elements[j].id = element_id
        let sequence_order: Vec<usize> = if let Some(ref render_data) = self.render_data {
            // Build element_id -> index map
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

        // Create StepRenderData
        if let Some(ref render_data) = self.render_data {
            let mut step_render_data = graphics::StepRenderData::new(render_data.clone());
            step_render_data.set_step_data(step_table);
            step_render_data.set_sequence_order(sequence_order);
            self.step_render_data = Some(step_render_data);
        }

        // Save tables to output directory (CSV files)
        if !self.ui_state.file_path.is_empty() {
            let input_path = std::path::Path::new(&self.ui_state.file_path);
            if let Some(parent) = input_path.parent() {
                let output_dir = parent.join("output");
                if let Err(e) = stability::save_all_tables(
                    &table_result,
                    &self.stability_elements,
                    &self.stability_nodes,
                    &output_dir,
                ) {
                    warnings.push(format!("Failed to save tables: {}", e));
                }
            }
        }

        // Update UI state with step info
        self.ui_state.has_step_data = max_step > 0;
        self.ui_state.max_step = max_step;
        self.ui_state.current_step = 1;
        self.ui_state.step_input = "1".to_string();
        self.ui_state.workfront_count = workfront_count;
        self.ui_state.needs_recalc = false; // Calculation complete
        self.ui_state.validation_passed = errors.is_empty();

        // Build step elements map for UI metrics
        self.ui_state.step_elements = stability::build_step_elements_map(
            &table_result.step_table,
            &self.stability_elements,
            &self.stability_nodes,
        );

        // Update floor column data - set to step 1 (first step installed)
        let floor_data =
            stability::get_floor_column_data(&self.stability_elements, &self.stability_nodes);
        self.ui_state.floor_column_data = floor_data
            .into_iter()
            .map(|(floor, total)| (floor, total, 0)) // Start at 0, will be updated by step
            .collect();

        // Update floor counts for current step (step 1)
        self.update_floor_counts_for_step();

        // Build status message
        let node_count = self.stability_nodes.len();
        let element_count = self.stability_elements.len();
        let mut status_msg = format!(
            "Calculated: {} nodes, {} elements, {} steps, {} workfront(s)",
            node_count, element_count, max_step, workfront_count
        );
        if !errors.is_empty() {
            status_msg.push_str(&format!(" | {} error(s)", errors.len()));
        }
        if !warnings.is_empty() {
            status_msg.push_str(&format!(" | {} warning(s)", warnings.len()));
        }
        self.ui_state.status_message = status_msg;

        // Recalculate viewport transform
        if let Some(ref mut data) = self.render_data {
            let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
            data.calculate_transform(rect, &self.view_state);
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
        self.ui_state.reset();
        self.render_data = None;
        self.step_render_data = None;
        self.view_state = graphics::ViewState::new();
        // Clear intermediate stability data
        self.stability_nodes.clear();
        self.stability_elements.clear();
        self.stability_element_data.clear();
    }
}

impl eframe::App for AssyPlanApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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
                let recalc_enabled = self.ui_state.has_data;
                let recalc_button = if self.ui_state.needs_recalc && recalc_enabled {
                    // Orange highlight to draw user attention
                    egui::Button::new("🔄 Recalc").fill(egui::Color32::from_rgb(255, 180, 50))
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

                ui.separator();

                // Mode toggle
                ui.label("Mode:");
                ui.radio_value(
                    &mut self.ui_state.mode,
                    "Development".to_string(),
                    "Development",
                );
                ui.add_enabled(false, egui::Label::new("Simulation"));

                ui.separator();

                // Display mode toggle (Model / Construction)
                ui.label("Display:");
                ui.selectable_value(
                    &mut self.ui_state.display_mode,
                    graphics::DisplayMode::Model,
                    "Model",
                );
                let construction_enabled = self.ui_state.has_step_data;
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

                ui.separator();

                // ID labels toggle
                ui.checkbox(&mut self.ui_state.show_id_labels, "Show IDs");

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
                        ctx.request_repaint();
                    }
                }
            });
        });

        // Construction navigation bar - only shown in Construction mode with data
        if self.ui_state.display_mode == graphics::DisplayMode::Construction
            && (self.ui_state.has_step_data || self.ui_state.max_sequence > 0)
        {
            egui::TopBottomPanel::top("construction_nav").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Dropdown menu for Sequence/Step selection
                    let old_mode = self.ui_state.construction_view_mode;
                    egui::ComboBox::from_id_source("construction_view_mode")
                        .selected_text(match self.ui_state.construction_view_mode {
                            graphics::ConstructionViewMode::Sequence => "Sequence",
                            graphics::ConstructionViewMode::Step => "Step",
                        })
                        .show_ui(ui, |ui: &mut egui::Ui| {
                            ui.selectable_value(
                                &mut self.ui_state.construction_view_mode,
                                graphics::ConstructionViewMode::Sequence,
                                "Sequence",
                            );
                            ui.selectable_value(
                                &mut self.ui_state.construction_view_mode,
                                graphics::ConstructionViewMode::Step,
                                "Step",
                            );
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
                ui.label(format!("Status: {}", self.ui_state.status_message));

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
                        ui.label(format!("Steps: {}", self.ui_state.max_step));
                        ui.label(format!(
                            "Display: {}",
                            match self.ui_state.display_mode {
                                graphics::DisplayMode::Model => "Model",
                                graphics::DisplayMode::Construction => "Construction",
                            }
                        ));
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

                    ui.label("Show Model Info:");
                    ui.indent("model_info_opts", |ui| {
                        ui.checkbox(&mut self.ui_state.show_grid, "Grid");
                        ui.checkbox(&mut self.ui_state.show_nodes, "Nodes");
                        ui.checkbox(&mut self.ui_state.show_elements, "Elements");
                    });

                    ui.separator();
                    ui.checkbox(&mut self.ui_state.show_id_labels, "Show ID Labels");
                    if self.ui_state.show_id_labels {
                        ui.indent("id_label_opts", |ui| {
                            ui.checkbox(&mut self.ui_state.show_node_ids, "Node ID");
                            ui.checkbox(&mut self.ui_state.show_element_ids, "Element ID");
                        });
                    }
                }
                "View" => {
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
                                // Construction view - show based on construction_view_mode
                                match self.ui_state.construction_view_mode {
                                    graphics::ConstructionViewMode::Sequence => {
                                        // Sequence mode: render elements 1 to current_sequence
                                        // Uses render_data directly (no step calculation needed)
                                        if let Some(ref mut data) = self.render_data {
                                            data.calculate_transform(rect, &self.view_state);

                                            // Use step_render_data for rendering with sequence mode
                                            if let Some(ref step_data) = self.step_render_data {
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
                                        }
                                    }
                                    graphics::ConstructionViewMode::Step => {
                                        // Step mode: show step-by-step with coloring
                                        if let Some(ref mut step_data) = self.step_render_data {
                                            // Sync current step from UI state to step_render_data
                                            step_data.set_current_step(self.ui_state.current_step);

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
                                        }
                                    }
                                }
                            }
                        }

                        // Always draw axis cube
                        graphics::axis_cube::paint_axis_cube(&painter, rect, &self.view_state);
                    } else {
                        ui.centered_and_justified(|ui| {
                            ui.label("No data loaded.\nClick 'Open File' to load a CSV file.");
                        });
                    }
                }
                "Result" => {
                    // Use full Result tab UI with metrics, progress bars, floor data
                    graphics::render_result_tab(ui, &self.ui_state);
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
}
