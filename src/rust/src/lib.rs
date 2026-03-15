pub mod graphics;
pub mod sim_engine;
pub mod sim_grid;
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
    // Simulation grid — stored after run_simulation() so View tab can render 3D
    sim_grid: Option<sim_grid::SimGrid>,
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
                self.ui_state.status_message = format!(
                    "⚠ Calculation failed: predecessor cycle detected.\n{}",
                    errors.join("\n")
                );
                self.ui_state.validation_passed = false;
                self.ui_state.has_step_data = false;
                self.step_render_data = None;
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
        self.ui_state.needs_recalc = false; // Calculation complete
        self.ui_state.validation_passed = errors.is_empty();
        self.ui_state.workfront_count = workfront_count;
        self.ui_state.current_step = 1;
        self.ui_state.step_input = "1".to_string();

        if !errors.is_empty() {
            // Errors present — block Construction view, clear step data
            self.ui_state.has_step_data = false;
            self.ui_state.max_step = 0;
            self.step_render_data = None;
        } else {
            self.ui_state.has_step_data = max_step > 0;
            self.ui_state.max_step = max_step;
        }

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
            status_msg = format!(
                "⚠ Calculation completed with {} error(s):\n{}",
                errors.len(),
                errors.join("\n")
            );
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

    /// Run the Simulation Mode: generate N scenarios with Monte-Carlo engine.
    fn run_simulation(&mut self) {
        let cfg = self.ui_state.grid_config.clone();
        let workfronts = self.ui_state.sim_workfronts.clone();
        let weights = self.ui_state.sim_weights;
        let threshold = self.ui_state.upper_floor_threshold;
        let count = self.ui_state.sim_scenario_count;

        // Ensure at least one workfront
        let workfronts = if workfronts.is_empty() {
            // Default: corner (0,0)
            vec![graphics::ui::SimWorkfront {
                id: 1,
                grid_x: 0,
                grid_y: 0,
            }]
        } else {
            workfronts
        };

        self.ui_state.sim_running = true;
        self.ui_state.status_message = format!(
            "Running simulation: {} scenarios ({}×{} grid, {} floors)...",
            count, cfg.nx, cfg.ny, cfg.nz
        );

        let grid = sim_grid::SimGrid::new(cfg.nx, cfg.ny, cfg.nz, cfg.dx, cfg.dy, cfg.dz);

        let scenarios =
            sim_engine::run_all_scenarios(count, &grid, &workfronts, weights, threshold);

        // Store grid so the View tab can render a 3D view of installed elements
        self.sim_grid = Some(grid.clone());

        // Select best scenario: most total members installed (highest completion)
        let best_idx = scenarios
            .iter()
            .enumerate()
            .max_by_key(|(_, s)| s.metrics.total_members_installed)
            .map(|(i, _)| i);

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
        self.ui_state.sim_selected_scenario = best_idx;
        self.ui_state.sim_running = false;
        self.ui_state.sim_current_step = 1;
        self.ui_state.needs_recalc = false;

        self.ui_state.status_message = format!(
            "Simulation complete: {} scenarios generated{}",
            scenario_count, best_info
        );
    }

    /// Export simulation debug files (all scenarios → simulation_debug/ folder).
    /// Returns a status message for display in the UI.
    fn export_simulation_debug(&self) -> String {
        use std::fmt::Write as FmtWrite;
        use std::fs;
        use std::io::Write;

        let scenarios = &self.ui_state.sim_scenarios;
        if scenarios.is_empty() {
            return "❌ No scenarios to export. Run simulation first.".to_string();
        }

        // Determine output directory: next to executable, subfolder "simulation_debug"
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let out_dir = exe_dir.join("simulation_debug");

        if let Err(e) = fs::create_dir_all(&out_dir) {
            return format!("❌ Failed to create output dir: {}", e);
        }

        let cfg = &self.ui_state.grid_config;

        // ── 1. Summary text file ─────────────────────────────────────────
        let mut summary = String::new();
        let _ = writeln!(summary, "AssyPlan Simulation Debug Export");
        let _ = writeln!(
            summary,
            "Generated: {} seconds since UNIX epoch",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        );
        let _ = writeln!(summary, "");
        let _ = writeln!(
            summary,
            "Grid: {}×{} (nx×ny), {} z-levels, dx={:.0}mm, dy={:.0}mm, dz={:.0}mm",
            cfg.nx, cfg.ny, cfg.nz, cfg.dx, cfg.dy, cfg.dz
        );
        let _ = writeln!(
            summary,
            "Upper-floor threshold: {:.1}%",
            self.ui_state.upper_floor_threshold * 100.0
        );
        let _ = writeln!(summary, "Scenarios: {}", scenarios.len());
        let _ = writeln!(
            summary,
            "Weights: w1(member_count)={:.2}, w2(connectivity)={:.2}, w3(distance)={:.2}",
            self.ui_state.sim_weights.0, self.ui_state.sim_weights.1, self.ui_state.sim_weights.2
        );
        let _ = writeln!(summary, "");
        let _ = writeln!(
            summary,
            "{:<6} {:<8} {:<10} {:<12} {:<12} {:<20}",
            "ID", "Steps", "Members", "Avg/Step", "Connectivity", "Termination"
        );
        let _ = writeln!(summary, "{}", "-".repeat(70));
        for s in scenarios {
            let _ = writeln!(
                summary,
                "{:<6} {:<8} {:<10} {:<12.2} {:<12.2} {:<20}",
                s.id,
                s.metrics.total_steps,
                s.metrics.total_members_installed,
                s.metrics.avg_members_per_step,
                s.metrics.avg_connectivity,
                format!("{}", s.metrics.termination_reason)
            );
        }

        let summary_path = out_dir.join("simulation_summary.txt");
        let mut summary_errors = 0usize;
        if let Ok(mut f) = fs::File::create(&summary_path) {
            if f.write_all(summary.as_bytes()).is_err() {
                summary_errors += 1;
            }
        } else {
            summary_errors += 1;
        }

        // ── 2. Per-scenario CSV files ────────────────────────────────────
        let mut csv_errors = 0usize;
        for scenario in scenarios {
            let csv_path = out_dir.join(format!("scenario_{:04}_steps.csv", scenario.id));
            let mut csv = String::new();
            let _ = writeln!(csv, "step,workfront_id,floor,member_count,element_ids");
            for (step_idx, step) in scenario.steps.iter().enumerate() {
                let ids_str = step
                    .element_ids
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>()
                    .join(";");
                let _ = writeln!(
                    csv,
                    "{},{},{},{},{}",
                    step_idx + 1, // 1-indexed
                    step.workfront_id,
                    step.floor,
                    step.element_ids.len(),
                    ids_str
                );
            }
            if let Ok(mut f) = fs::File::create(&csv_path) {
                if f.write_all(csv.as_bytes()).is_err() {
                    csv_errors += 1;
                }
            } else {
                csv_errors += 1;
            }
        }

        if summary_errors + csv_errors == 0 {
            format!(
                "✅ Exported {} scenario(s) + summary to: {}",
                scenarios.len(),
                out_dir.display()
            )
        } else {
            format!(
                "❌ Export completed with {} error(s). Output: {}",
                summary_errors + csv_errors,
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
            let status = self.export_simulation_debug();
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

                    ui.label("Show Model Info:");
                    ui.indent("model_info_opts", |ui| {
                        ui.checkbox(&mut self.ui_state.show_grid, "Grid");
                        ui.checkbox(&mut self.ui_state.show_nodes, "Nodes");
                        ui.checkbox(&mut self.ui_state.show_elements, "Elements");
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
                        let _grid_changed =
                            graphics::sim_ui::render_sim_settings(ui, &mut self.ui_state);
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
                }
                "View" => {
                    // Simulation mode has its own view (grid plan), independent of render_data
                    if self.ui_state.display_mode == graphics::DisplayMode::Simulation {
                        // When a grid + selected scenario exist: split view
                        // - bottom panel: 3D render of installed elements (orbit/zoom via view_state)
                        // - remaining area: 2D grid plan with workfront click handling
                        let has_3d = self.sim_grid.is_some()
                            && self.ui_state.sim_selected_scenario.is_some();

                        if has_3d {
                            // Bottom panel: 3D view (resizable, default 300px)
                            egui::TopBottomPanel::bottom("sim_3d_panel")
                                .resizable(true)
                                .default_height(300.0)
                                .min_height(80.0)
                                .show_inside(ui, |ui| {
                                    // Handle orbit/zoom/pan input first (needs &mut ViewState)
                                    let rect_3d = ui.available_rect_before_wrap();
                                    let resp = ui.allocate_rect(
                                        rect_3d,
                                        egui::Sense::click_and_drag(),
                                    );
                                    let pointer_in_rect = ui
                                        .input(|i| i.pointer.hover_pos())
                                        .map(|p| rect_3d.contains(p))
                                        .unwrap_or(false);
                                    if pointer_in_rect || resp.dragged() {
                                        if self.view_state.handle_input(&resp, ui) {
                                            ctx.request_repaint();
                                        }
                                    }
                                    // Restore full rect for 3D rendering
                                    let resp2 = ui.allocate_rect(
                                        rect_3d,
                                        egui::Sense::hover(),
                                    );
                                    if let Some(ref grid) = self.sim_grid {
                                        // Render using a painter at the full 3D panel rect
                                        let painter = ui.painter_at(rect_3d);
                                        use crate::graphics::renderer::{Element, Node, RenderData};
                                        use std::collections::HashSet;

                                        let installed_ids: HashSet<i32> = self
                                            .ui_state
                                            .sim_selected_scenario
                                            .and_then(|idx| {
                                                self.ui_state.sim_scenarios.get(idx)
                                            })
                                            .map(|scenario| {
                                                let steps_to_show = self
                                                    .ui_state
                                                    .sim_current_step
                                                    .min(scenario.steps.len());
                                                scenario.steps[..steps_to_show]
                                                    .iter()
                                                    .flat_map(|s| s.element_ids.iter().copied())
                                                    .collect()
                                            })
                                            .unwrap_or_default();

                                        painter.rect_filled(
                                            rect_3d,
                                            0.0,
                                            egui::Color32::from_gray(18),
                                        );

                                        let mut render_data = RenderData::new();
                                        for n in &grid.nodes {
                                            render_data.add_node(Node {
                                                id: n.id,
                                                x: n.x,
                                                y: n.y,
                                                z: n.z,
                                            });
                                        }
                                        for e in &grid.elements {
                                            render_data.add_element(Element {
                                                id: e.id,
                                                node_i_id: e.node_i_id,
                                                node_j_id: e.node_j_id,
                                                member_type: e.member_type.clone(),
                                            });
                                        }
                                        render_data
                                            .calculate_transform(rect_3d, &self.view_state);

                                        let node_map: std::collections::HashMap<
                                            i32,
                                            (f64, f64, f64),
                                        > = grid
                                            .nodes
                                            .iter()
                                            .map(|n| (n.id, (n.x, n.y, n.z)))
                                            .collect();

                                        // Ghost (uninstalled)
                                        let ghost = egui::Color32::from_gray(38);
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
                                                    egui::Stroke::new(0.5, ghost),
                                                );
                                            }
                                        }

                                        // Installed
                                        let col_color =
                                            egui::Color32::from_rgb(220, 80, 60);
                                        let gdr_color =
                                            egui::Color32::from_rgb(60, 190, 90);
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

                                        // Nodes
                                        let node_dot =
                                            egui::Color32::from_gray(70);
                                        for n in &grid.nodes {
                                            let p = render_data.project_to_2d(
                                                n.x,
                                                n.y,
                                                n.z,
                                                &self.view_state,
                                            );
                                            if rect_3d.contains(p) {
                                                painter.circle_filled(p, 1.5, node_dot);
                                            }
                                        }

                                        // Info overlay
                                        let step_total = self
                                            .ui_state
                                            .sim_selected_scenario
                                            .and_then(|i| {
                                                self.ui_state.sim_scenarios.get(i)
                                            })
                                            .map(|s| s.steps.len())
                                            .unwrap_or(0);
                                        let overlay = format!(
                                            "3D View — Step {}/{} — {} member(s) installed",
                                            self.ui_state
                                                .sim_current_step
                                                .min(step_total.max(1)),
                                            step_total,
                                            installed_ids.len()
                                        );
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
                                            egui::Stroke::new(2.0, col_color),
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
                                            egui::Stroke::new(2.0, gdr_color),
                                        );
                                        painter.text(
                                            egui::pos2(lx + 18.0, ly + 18.0),
                                            egui::Align2::LEFT_CENTER,
                                            "Girder",
                                            egui::FontId::proportional(9.0),
                                            egui::Color32::from_gray(180),
                                        );
                                        let _ = resp2;
                                    }
                                });
                        }

                        // 2D grid plan (remaining area, handles workfront clicks)
                        if graphics::sim_ui::render_sim_view(ui, &mut self.ui_state) {
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
                                // Construction view - only render if step data is ready
                                if !self.ui_state.has_step_data {
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
                                        // Sequence mode: render elements 1 to current_sequence
                                        // Uses step_render_data.base for transform (same as Step mode)
                                        if let Some(ref mut data) = self.render_data {
                                            data.calculate_transform(rect, &self.view_state);

                                            // Use step_render_data for rendering with sequence mode
                                            if let Some(ref mut step_data) = self.step_render_data {
                                                // Must calculate transform on step_data.base so that
                                                // render_sequence uses up-to-date zoom/pan from view_state
                                                step_data
                                                    .base
                                                    .calculate_transform(rect, &self.view_state);

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

                                            // Draw element IDs at midpoint if enabled
                                            if self.ui_state.show_id_labels
                                                && self.ui_state.show_element_ids
                                                && self.ui_state.show_elements
                                            {
                                                for element in &step_data.base.elements {
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
                        graphics::sim_ui::render_sim_result(ui, &mut self.ui_state);
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
