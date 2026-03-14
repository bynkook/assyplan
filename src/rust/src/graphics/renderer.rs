// Renderer module for 2D visualization

use eframe::egui;

/// Represents a 3D node in the structure
#[derive(Clone, Debug)]
pub struct Node {
    pub id: i32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// Represents a structural element connecting two nodes
#[derive(Clone, Debug)]
pub struct Element {
    pub id: i32,
    pub node_i_id: i32,
    pub node_j_id: i32,
    pub member_type: String, // "Column" or "Girder"
}

/// Render data container for visualization
pub struct RenderData {
    pub nodes: Vec<Node>,
    pub elements: Vec<Element>,
    pub scale: f32,
    pub offset: egui::Vec2,
}

impl RenderData {
    /// Create a new RenderData instance
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            elements: Vec::new(),
            scale: 1.0,
            offset: egui::Vec2::ZERO,
        }
    }

    /// Add a node to the render data
    pub fn add_node(&mut self, node: Node) {
        self.nodes.push(node);
    }

    /// Add an element to the render data
    pub fn add_element(&mut self, element: Element) {
        self.elements.push(element);
    }

    /// Calculate optimal scale and offset to fit all nodes in the viewport
    pub fn calculate_transform(&mut self, rect: egui::Rect) {
        if self.nodes.is_empty() {
            self.scale = 1.0;
            self.offset = egui::Vec2::ZERO;
            return;
        }

        // Find bounds of all nodes (project to z=min(z) for 2D view)
        let min_x = self.nodes.iter().map(|n| n.x).fold(f64::INFINITY, f64::min);
        let max_x = self
            .nodes
            .iter()
            .map(|n| n.x)
            .fold(f64::NEG_INFINITY, f64::max);
        let min_y = self.nodes.iter().map(|n| n.y).fold(f64::INFINITY, f64::min);
        let max_y = self
            .nodes
            .iter()
            .map(|n| n.y)
            .fold(f64::NEG_INFINITY, f64::max);

        let data_width = max_x - min_x;
        let data_height = max_y - min_y;

        if data_width <= 0.0 || data_height <= 0.0 {
            self.scale = 1.0;
            self.offset = egui::Vec2::new(rect.left(), rect.top());
            return;
        }

        // Calculate scale to fit in viewport with padding
        let padding = 50.0f32;
        let view_width = rect.width() - 2.0 * padding;
        let view_height = rect.height() - 2.0 * padding;

        let scale_x = view_width as f64 / data_width;
        let scale_y = view_height as f64 / data_height;
        self.scale = (scale_x.min(scale_y) as f32).min(10.0).max(0.01);

        // Calculate offset to center the data
        let scaled_width = data_width * self.scale as f64;
        let scaled_height = data_height * self.scale as f64;
        let offset_x = (view_width as f64 - scaled_width) / 2.0 + padding as f64;
        let offset_y = (view_height as f64 - scaled_height) / 2.0 + padding as f64;

        self.offset = egui::Vec2::new(rect.left() + offset_x as f32, rect.top() + offset_y as f32);
    }

    /// Transform a 3D point to 2D screen coordinates (project to min z)
    pub fn project_to_2d(&self, x: f64, y: f64, _z: f64) -> egui::Pos2 {
        let min_z = self.nodes.iter().map(|n| n.z).fold(0.0, f64::min);
        egui::Pos2::new(
            self.offset.x + (x * self.scale as f64) as f32,
            self.offset.y + ((y - min_z) * self.scale as f64) as f32, // Flip y for screen coords
        )
    }

    /// Main render function that calls all sub-renderers
    pub fn render(&self, painter: &egui::Painter, rect: egui::Rect) {
        self.render_grid(painter, rect);
        self.render_nodes(painter, rect);
        self.render_elements(painter, rect);
    }

    /// Render grid lines at z=min(z) level
    fn render_grid(&self, painter: &egui::Painter, rect: egui::Rect) {
        if self.nodes.is_empty() {
            return;
        }

        // Find bounds
        let min_x = self.nodes.iter().map(|n| n.x).fold(f64::INFINITY, f64::min);
        let max_x = self
            .nodes
            .iter()
            .map(|n| n.x)
            .fold(f64::NEG_INFINITY, f64::max);
        let min_y = self.nodes.iter().map(|n| n.y).fold(f64::INFINITY, f64::min);
        let max_y = self
            .nodes
            .iter()
            .map(|n| n.y)
            .fold(f64::NEG_INFINITY, f64::max);

        let grid_size = 1000.0; // Grid spacing

        // Vertical grid lines
        let start_x = (min_x / grid_size).floor() * grid_size;
        let end_x = (max_x / grid_size).ceil() * grid_size;
        let mut x = start_x;
        while x <= end_x {
            let p1 = self.project_to_2d(x, min_y, 0.0);
            let p2 = self.project_to_2d(x, max_y, 0.0);
            if rect.contains(p1) || rect.contains(p2) {
                painter.line_segment(
                    [p1, p2],
                    egui::Stroke::new(1.0, egui::Color32::from_gray(100)),
                );
            }
            x += grid_size;
        }

        // Horizontal grid lines
        let start_y = (min_y / grid_size).floor() * grid_size;
        let end_y = (max_y / grid_size).ceil() * grid_size;
        let mut y = start_y;
        while y <= end_y {
            let p1 = self.project_to_2d(min_x, y, 0.0);
            let p2 = self.project_to_2d(max_x, y, 0.0);
            if rect.contains(p1) || rect.contains(p2) {
                painter.line_segment(
                    [p1, p2],
                    egui::Stroke::new(1.0, egui::Color32::from_gray(100)),
                );
            }
            y += grid_size;
        }
    }

    /// Render nodes as small dots
    fn render_nodes(&self, painter: &egui::Painter, rect: egui::Rect) {
        for node in &self.nodes {
            let pos = self.project_to_2d(node.x, node.y, node.z);
            if rect.contains(pos) {
                // Draw a small circle for each node
                let radius = 3.0;
                painter.circle_filled(pos, radius, egui::Color32::from_rgb(0, 100, 200));
            }
        }
    }

    /// Render elements as lines connecting nodes
    fn render_elements(&self, painter: &egui::Painter, rect: egui::Rect) {
        for element in &self.elements {
            // Find the start and end nodes
            let node_i = self.nodes.iter().find(|n| n.id == element.node_i_id);
            let node_j = self.nodes.iter().find(|n| n.id == element.node_j_id);

            if let (Some(ni), Some(nj)) = (node_i, node_j) {
                let p1 = self.project_to_2d(ni.x, ni.y, ni.z);
                let p2 = self.project_to_2d(nj.x, nj.y, nj.z);

                // Only draw if at least one point is in rect
                if rect.contains(p1) || rect.contains(p2) {
                    // Different color based on member type
                    let color = if element.member_type == "Column" {
                        egui::Color32::from_rgb(200, 50, 50) // Red for columns
                    } else {
                        egui::Color32::from_rgb(50, 150, 50) // Green for girders
                    };
                    painter.line_segment([p1, p2], egui::Stroke::new(2.0, color));
                }
            }
        }
    }
}

impl Default for RenderData {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_struct() {
        let node = Node {
            id: 1,
            x: 1000.0,
            y: 2000.0,
            z: 3000.0,
        };
        assert_eq!(node.id, 1);
        assert_eq!(node.x, 1000.0);
    }

    #[test]
    fn test_element_struct() {
        let element = Element {
            id: 1,
            node_i_id: 1,
            node_j_id: 2,
            member_type: "Column".to_string(),
        };
        assert_eq!(element.member_type, "Column");
    }

    #[test]
    fn test_render_data_new() {
        let render_data = RenderData::new();
        assert!(render_data.nodes.is_empty());
        assert!(render_data.elements.is_empty());
        assert_eq!(render_data.scale, 1.0);
    }

    #[test]
    fn test_render_data_add_node() {
        let mut render_data = RenderData::new();
        let node = Node {
            id: 1,
            x: 1000.0,
            y: 2000.0,
            z: 3000.0,
        };
        render_data.add_node(node);
        assert_eq!(render_data.nodes.len(), 1);
    }

    #[test]
    fn test_render_data_add_element() {
        let mut render_data = RenderData::new();
        let element = Element {
            id: 1,
            node_i_id: 1,
            node_j_id: 2,
            member_type: "Column".to_string(),
        };
        render_data.add_element(element);
        assert_eq!(render_data.elements.len(), 1);
    }
}
