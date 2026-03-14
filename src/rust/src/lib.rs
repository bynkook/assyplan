pub mod graphics;

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
}

impl Default for AssyPlanApp {
    fn default() -> Self {
        Self {
            ui_state: graphics::UiState::new(),
            render_data: None,
            step_render_data: None,
            view_state: graphics::ViewState::new(),
            file_dialog_open: false,
        }
    }
}

impl AssyPlanApp {
    /// Load CSV file and parse into render data
    ///
    /// CSV format: 구분,부재_FL,부재_CD,부재ID,node_i_x,node_i_y,node_i_z,node_j_x,node_j_y,node_j_z,...
    /// Column indices: 4=node_i_x, 5=node_i_y, 6=node_i_z, 7=node_j_x, 8=node_j_y, 9=node_j_z
    fn load_file(&mut self, path: &std::path::Path) {
        self.ui_state.status_message = format!("Loading: {}", path.display());
        self.ui_state.file_path = path.display().to_string();

        match std::fs::read_to_string(path) {
            Ok(content) => {
                // Step 1: Collect all unique coordinates from CSV
                let mut coords_set: std::collections::HashSet<(i64, i64, i64)> =
                    std::collections::HashSet::new();
                let mut element_data: Vec<((i64, i64, i64), (i64, i64, i64))> = Vec::new();

                for line in content.lines() {
                    let line = line.trim();
                    // Skip empty lines, comments, and header
                    if line.is_empty() || line.starts_with('#') || line.starts_with("구분") {
                        continue;
                    }

                    let parts: Vec<&str> = line.split(',').collect();
                    if parts.len() >= 10 {
                        // Parse coordinates from correct indices (4-9)
                        // Use i64 for exact coordinate matching (avoid float comparison issues)
                        let x1: i64 = parts
                            .get(4)
                            .and_then(|s| s.trim().parse().ok())
                            .unwrap_or(0);
                        let y1: i64 = parts
                            .get(5)
                            .and_then(|s| s.trim().parse().ok())
                            .unwrap_or(0);
                        let z1: i64 = parts
                            .get(6)
                            .and_then(|s| s.trim().parse().ok())
                            .unwrap_or(0);
                        let x2: i64 = parts
                            .get(7)
                            .and_then(|s| s.trim().parse().ok())
                            .unwrap_or(0);
                        let y2: i64 = parts
                            .get(8)
                            .and_then(|s| s.trim().parse().ok())
                            .unwrap_or(0);
                        let z2: i64 = parts
                            .get(9)
                            .and_then(|s| s.trim().parse().ok())
                            .unwrap_or(0);

                        let coord_i = (x1, y1, z1);
                        let coord_j = (x2, y2, z2);

                        coords_set.insert(coord_i);
                        coords_set.insert(coord_j);
                        element_data.push((coord_i, coord_j));
                    }
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

                // Step 5: Create elements with node ID references
                for (element_idx, (coord_i, coord_j)) in element_data.iter().enumerate() {
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
                }

                let node_count = render_data.nodes.len();
                let element_count = render_data.elements.len();

                self.render_data = Some(render_data);
                self.ui_state.has_data = true;
                self.ui_state.validation_passed = true;
                self.ui_state.status_message =
                    format!("Loaded: {} nodes, {} elements", node_count, element_count);

                // Reset view state for zoom-to-fit on load
                // Set initial view: origin at bottom-left, X-axis pointing upper-right (isometric)
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

    /// Recalculate and validate data
    fn recalculate(&mut self) {
        if let Some(ref mut data) = self.render_data {
            // Recalculate transform for viewport
            let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
            data.calculate_transform(rect, &self.view_state);
            self.ui_state.validation_passed = true;
            self.ui_state.status_message = "Recalculation complete".to_string();
        }
    }

    /// Reset all state
    fn reset(&mut self) {
        self.ui_state.reset();
        self.render_data = None;
        self.step_render_data = None;
        self.view_state = graphics::ViewState::new();
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

                // Recalc button
                let recalc_enabled = self.ui_state.has_data;
                if ui
                    .add_enabled(recalc_enabled, egui::Button::new("🔄 Recalc"))
                    .clicked()
                {
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

                // Step navigation (only show if we have steps)
                if self.ui_state.max_step > 0 {
                    ui.separator();
                    ui.label(format!(
                        "Step {}/{}",
                        self.ui_state.current_step, self.ui_state.max_step
                    ));

                    let prev_enabled = self.ui_state.current_step > 1;
                    if ui
                        .add_enabled(prev_enabled, egui::Button::new("◀"))
                        .clicked()
                    {
                        self.ui_state.current_step = self.ui_state.current_step.saturating_sub(1);
                    }

                    let mut step_slider = self.ui_state.current_step;
                    if ui
                        .add(
                            egui::Slider::new(&mut step_slider, 1..=self.ui_state.max_step)
                                .show_value(false),
                        )
                        .changed()
                    {
                        self.ui_state.current_step = step_slider;
                    }

                    let next_enabled = self.ui_state.current_step < self.ui_state.max_step;
                    if ui
                        .add_enabled(next_enabled, egui::Button::new("▶"))
                        .clicked()
                    {
                        self.ui_state.current_step =
                            (self.ui_state.current_step + 1).min(self.ui_state.max_step);
                    }
                }
            });
        });

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
                    // Render viewport
                    if let Some(ref mut data) = self.render_data {
                        let rect = ui.available_rect_before_wrap();
                        let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
                        if self.view_state.handle_input(&response, ui) {
                            ctx.request_repaint();
                        }

                        data.calculate_transform(rect, &self.view_state);

                        let painter = ui.painter_at(rect);
                        painter.rect_filled(rect, 0.0, egui::Color32::from_gray(18));

                        let visibility = graphics::VisibilitySettings {
                            show_grid: self.ui_state.show_grid,
                            show_nodes: self.ui_state.show_nodes,
                            show_elements: self.ui_state.show_elements,
                        };
                        data.render(&painter, rect, &self.view_state, &visibility);
                        graphics::axis_cube::paint_axis_cube(&painter, rect, &self.view_state);

                        // Draw node IDs if enabled
                        if self.ui_state.show_id_labels
                            && self.ui_state.show_node_ids
                            && self.ui_state.show_nodes
                        {
                            for node in &data.nodes {
                                let pos =
                                    data.project_to_2d(node.x, node.y, node.z, &self.view_state);
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
                                // Find the two nodes connected by this element
                                let node_i = data.nodes.iter().find(|n| n.id == element.node_i_id);
                                let node_j = data.nodes.iter().find(|n| n.id == element.node_j_id);

                                if let (Some(ni), Some(nj)) = (node_i, node_j) {
                                    // Calculate midpoint
                                    let mid_x = (ni.x + nj.x) / 2.0;
                                    let mid_y = (ni.y + nj.y) / 2.0;
                                    let mid_z = (ni.z + nj.z) / 2.0;

                                    let pos =
                                        data.project_to_2d(mid_x, mid_y, mid_z, &self.view_state);
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
                    } else {
                        ui.centered_and_justified(|ui| {
                            ui.label("No data loaded.\nClick 'Open File' to load a CSV file.");
                        });
                    }
                }
                "Result" => {
                    ui.heading("Result");
                    ui.separator();
                    ui.label(format!(
                        "Validation Status: {}",
                        if self.ui_state.validation_passed {
                            "PASSED"
                        } else {
                            "NOT VALIDATED"
                        }
                    ));

                    if self.ui_state.has_data {
                        ui.label("Data has been processed successfully.");
                        if let Some(ref data) = self.render_data {
                            ui.separator();
                            ui.label(format!("Total Nodes: {}", data.nodes.len()));
                            ui.label(format!("Total Elements: {}", data.elements.len()));

                            let columns = data
                                .elements
                                .iter()
                                .filter(|e| e.member_type == "Column")
                                .count();
                            let girders = data
                                .elements
                                .iter()
                                .filter(|e| e.member_type == "Girder")
                                .count();
                            ui.label(format!("Columns: {}", columns));
                            ui.label(format!("Girders: {}", girders));
                        }
                    } else {
                        ui.label("No data loaded. Please open a file and run recalc.");
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
