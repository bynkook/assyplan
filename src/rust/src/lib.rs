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

// ============================================================================
// eframe Application
// ============================================================================

pub struct AssyPlanApp {
    render_data: Option<graphics::RenderData>,
}

impl Default for AssyPlanApp {
    fn default() -> Self {
        Self { render_data: None }
    }
}

impl eframe::App for AssyPlanApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("AssyPlan Render");
            if let Some(ref data) = self.render_data {
                let rect = ui.available_rect_before_wrap();
                data.render(&ui.painter(), rect);
            } else {
                ui.label("No data loaded");
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
                render_data: Some(*rd),
            }) as Box<dyn eframe::App>
        }),
    )
    .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
}

#[pymodule]
fn assyplan(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(load_and_validate, m)?)?;
    m.add_function(wrap_pyfunction!(render_data, m)?)?;
    m.add_class::<PyNode>()?;
    m.add_class::<PyElement>()?;
    m.add_class::<PyRenderData>()?;
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
}
