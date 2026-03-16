// Renderer module for 2D/3D visualization

use std::collections::BTreeSet;

use eframe::egui;

use super::view_state::ViewState;

/// Visibility settings for rendering
#[derive(Clone, Copy, Debug)]
pub struct VisibilitySettings {
    pub show_grid: bool,
    pub show_nodes: bool,
    pub show_elements: bool,
    /// Show hidden (inactive/uninstalled) nodes and elements in ghost style.
    /// Only relevant in Construction display mode; ignored in Model mode.
    pub show_hidden: bool,
}

impl Default for VisibilitySettings {
    fn default() -> Self {
        Self {
            show_grid: true,
            show_nodes: true,
            show_elements: true,
            show_hidden: true,
        }
    }
}

// ── Shared rendering style constants ──────────────────────────────────────────
/// Active node: blue dot, same as Dev Model view
pub const ACTIVE_NODE_RADIUS: f32 = 2.0;
pub const ACTIVE_NODE_COLOR: egui::Color32 = egui::Color32::from_rgb(0, 100, 200);

/// Ghost (inactive/uninstalled) element line style
pub const GHOST_COLOR: egui::Color32 = egui::Color32::from_gray(90);
pub const GHOST_STROKE_WIDTH: f32 = 0.5;

/// Ghost (inactive) node dot
pub const GHOST_NODE_RADIUS: f32 = 1.5;
pub const GHOST_NODE_COLOR: egui::Color32 = egui::Color32::from_gray(70);

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
    pub member_type: String,
}

/// Render data container for visualization
#[derive(Clone, Debug)]
pub struct RenderData {
    pub nodes: Vec<Node>,
    pub elements: Vec<Element>,
    pub scale: f32,
    pub offset: egui::Vec2,

    // Fit-related cached values (only updated on fit events)
    base_scale: f32,
    scene_center: [f64; 3],
    scene_depth: f32,
    projected_center: egui::Vec2, // 2D center of projected data
    last_fit_rect: Option<egui::Rect>,
    fit_dirty: bool,
}

impl RenderData {
    /// Create a new RenderData instance
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            elements: Vec::new(),
            scale: 1.0,
            offset: egui::Vec2::ZERO,
            base_scale: 1.0,
            scene_center: [0.0, 0.0, 0.0],
            scene_depth: 1000.0,
            projected_center: egui::Vec2::ZERO,
            last_fit_rect: None,
            fit_dirty: true,
        }
    }

    /// Add a node to the render data
    pub fn add_node(&mut self, node: Node) {
        self.nodes.push(node);
        self.fit_dirty = true;
    }

    /// Add an element to the render data
    pub fn add_element(&mut self, element: Element) {
        self.elements.push(element);
    }

    /// Mark fit as dirty (needs recalculation)
    pub fn invalidate_fit(&mut self) {
        self.fit_dirty = true;
    }

    /// Get scene depth for perspective calculations
    pub fn get_scene_depth(&self) -> f32 {
        self.scene_depth
    }

    /// Get scene center for perspective calculations
    pub fn get_scene_center(&self) -> [f64; 3] {
        self.scene_center
    }

    /// Calculate scene center and bounds from nodes (call once after loading)
    fn calculate_scene_bounds(&mut self) {
        if self.nodes.is_empty() {
            self.scene_center = [0.0, 0.0, 0.0];
            self.scene_depth = 1000.0;
            return;
        }

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
        let min_z = self.nodes.iter().map(|n| n.z).fold(f64::INFINITY, f64::min);
        let max_z = self
            .nodes
            .iter()
            .map(|n| n.z)
            .fold(f64::NEG_INFINITY, f64::max);

        self.scene_center = [
            (min_x + max_x) * 0.5,
            (min_y + max_y) * 0.5,
            (min_z + max_z) * 0.5,
        ];

        // Scene depth is the diagonal of the bounding box
        let dx = (max_x - min_x) as f32;
        let dy = (max_y - min_y) as f32;
        let dz = (max_z - min_z) as f32;
        self.scene_depth = (dx * dx + dy * dy + dz * dz).sqrt().max(100.0);
    }

    /// Update fit (base_scale) only when needed
    fn update_fit_if_needed(&mut self, rect: egui::Rect, view_state: &ViewState) {
        // Check if fit needs update
        let rect_changed = self.last_fit_rect.map_or(true, |r| {
            (r.width() - rect.width()).abs() > 1.0 || (r.height() - rect.height()).abs() > 1.0
        });

        if !self.fit_dirty && !rect_changed {
            return;
        }

        if self.nodes.is_empty() {
            self.base_scale = 1.0;
            self.fit_dirty = false;
            self.last_fit_rect = Some(rect);
            return;
        }

        // Calculate scene bounds if dirty
        if self.fit_dirty {
            self.calculate_scene_bounds();
        }

        // Project all nodes to get 2D extents
        let projected: Vec<egui::Vec2> = self
            .nodes
            .iter()
            .map(|node| {
                // Project relative to scene center for better perspective behavior
                let rel_point = [
                    (node.x - self.scene_center[0]) as f32,
                    (node.y - self.scene_center[1]) as f32,
                    (node.z - self.scene_center[2]) as f32,
                ];
                view_state.project_3d_to_2d_ortho(rel_point).0
            })
            .collect();

        let min_x = projected.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
        let max_x = projected
            .iter()
            .map(|p| p.x)
            .fold(f32::NEG_INFINITY, f32::max);
        let min_y = projected.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
        let max_y = projected
            .iter()
            .map(|p| p.y)
            .fold(f32::NEG_INFINITY, f32::max);

        let data_width = (max_x - min_x).max(1.0);
        let data_height = (max_y - min_y).max(1.0);

        // Calculate the 2D center of projected data (relative to scene center)
        let data_center_x = (min_x + max_x) * 0.5;
        let data_center_y = (min_y + max_y) * 0.5;

        let padding = 80.0f32; // Increased padding for better margins
        let view_width = (rect.width() - 2.0 * padding).max(1.0);
        let view_height = (rect.height() - 2.0 * padding).max(1.0);

        self.base_scale = (view_width / data_width).min(view_height / data_height);

        // Store projected data center for offset calculation
        self.projected_center = egui::vec2(data_center_x, data_center_y);

        self.fit_dirty = false;
        self.last_fit_rect = Some(rect);
    }

    /// Calculate transform for rendering (called every frame)
    pub fn calculate_transform(&mut self, rect: egui::Rect, view_state: &ViewState) {
        // Update fit only when needed
        self.update_fit_if_needed(rect, view_state);

        if self.nodes.is_empty() {
            self.scale = 1.0;
            self.offset = rect.center().to_vec2();
            return;
        }

        // Final scale = base_scale * user zoom
        // No clamp - base_scale already fits the data, zoom just multiplies
        self.scale = self.base_scale * view_state.scale_factor();

        // Calculate offset to center the projected data in the viewport
        // The projected_center is in projected coordinates (relative to scene center)
        // We want this center to appear at rect.center()
        self.offset = rect.center().to_vec2() + view_state.pan_offset
            - egui::vec2(
                self.projected_center.x * self.scale,
                -self.projected_center.y * self.scale, // Y is flipped in screen coords
            );
    }

    /// Transform a 3D point to 2D screen coordinates
    pub fn project_to_2d(&self, x: f64, y: f64, z: f64, view_state: &ViewState) -> egui::Pos2 {
        // Project relative to scene center
        let rel_point = [
            (x - self.scene_center[0]) as f32,
            (y - self.scene_center[1]) as f32,
            (z - self.scene_center[2]) as f32,
        ];
        let (projected, _) = view_state.project_3d_to_2d(rel_point, self.scene_depth);
        egui::pos2(
            self.offset.x + projected.x * self.scale,
            self.offset.y - projected.y * self.scale,
        )
    }

    /// Main render function that calls all sub-renderers
    pub fn render(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        view_state: &ViewState,
        visibility: &VisibilitySettings,
    ) {
        if visibility.show_grid {
            self.render_grid(painter, rect, view_state);
        }
        if visibility.show_nodes {
            self.render_nodes(painter, rect, view_state);
        }
        if visibility.show_elements {
            self.render_elements(painter, rect, view_state);
        }
    }

    /// Render grid lines on the current view plane (Revit-style)
    /// Grid lines are drawn at unique X and Y node coordinates on the min_z plane
    /// with numbered bubble markers at grid ends
    pub fn render_grid(&self, painter: &egui::Painter, _rect: egui::Rect, view_state: &ViewState) {
        if self.nodes.is_empty() {
            return;
        }

        // Collect unique X and Y coordinates using BTreeSet for sorted order
        // Use i64 keys (coordinate * 10) to handle floating point comparison
        let unique_x: BTreeSet<i64> = self
            .nodes
            .iter()
            .map(|n| (n.x * 10.0).round() as i64)
            .collect();
        let unique_y: BTreeSet<i64> = self
            .nodes
            .iter()
            .map(|n| (n.y * 10.0).round() as i64)
            .collect();

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
        let min_z = self.nodes.iter().map(|n| n.z).fold(f64::INFINITY, f64::min);

        // Grid line stroke
        let stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(120));

        // Extension distance for grid lines beyond data bounds (for bubble marker placement)
        let extend_dist = (max_x - min_x).max(max_y - min_y) * 0.15;

        // Bubble marker settings
        let bubble_radius = 12.0_f32;
        let bubble_stroke = egui::Stroke::new(1.5, egui::Color32::from_gray(100));
        let text_color = egui::Color32::from_gray(80);

        // Draw X-direction grid lines (vertical lines in plan view)
        // These run parallel to Y axis at each unique X coordinate
        for (idx, &x_key) in unique_x.iter().enumerate() {
            let x = x_key as f64 / 10.0;
            let p1 = self.project_to_2d(x, min_y - extend_dist, min_z, view_state);
            let p2 = self.project_to_2d(x, max_y + extend_dist, min_z, view_state);

            // Shrink line start by bubble_radius so it does not penetrate the circle marker
            let dir = (p2 - p1).normalized();
            let p1_trimmed = p1 + dir * bubble_radius;

            // Draw grid line (trimmed at bubble end)
            painter.line_segment([p1_trimmed, p2], stroke);

            // Draw bubble marker at the extended end (bottom side, min_y - extend_dist)
            let bubble_pos = p1;
            self.draw_grid_bubble(
                painter,
                bubble_pos,
                idx + 1,
                bubble_radius,
                bubble_stroke,
                text_color,
            );
        }

        // Draw Y-direction grid lines (horizontal lines in plan view)
        // These run parallel to X axis at each unique Y coordinate
        // Use letters A, B, C... for Y-direction grids (Revit convention)
        for (idx, &y_key) in unique_y.iter().enumerate() {
            let y = y_key as f64 / 10.0;
            let p1 = self.project_to_2d(min_x - extend_dist, y, min_z, view_state);
            let p2 = self.project_to_2d(max_x + extend_dist, y, min_z, view_state);

            // Shrink line start by bubble_radius so it does not penetrate the circle marker
            let dir = (p2 - p1).normalized();
            let p1_trimmed = p1 + dir * bubble_radius;

            // Draw grid line (trimmed at bubble end)
            painter.line_segment([p1_trimmed, p2], stroke);

            // Draw bubble marker at the extended end (left side, min_x - extend_dist)
            let bubble_pos = p1;
            let label = index_to_letter(idx);
            self.draw_grid_bubble_text(
                painter,
                bubble_pos,
                &label,
                bubble_radius,
                bubble_stroke,
                text_color,
            );
        }
    }

    /// Draw a numbered bubble marker for grid lines
    fn draw_grid_bubble(
        &self,
        painter: &egui::Painter,
        pos: egui::Pos2,
        number: usize,
        radius: f32,
        stroke: egui::Stroke,
        text_color: egui::Color32,
    ) {
        // Draw circle
        painter.circle_stroke(pos, radius, stroke);

        // Draw number text centered in bubble
        let text = format!("{}", number);
        let font_id = egui::FontId::proportional(radius * 1.2);
        let galley = painter.layout_no_wrap(text, font_id, text_color);
        let text_pos = egui::pos2(pos.x - galley.size().x / 2.0, pos.y - galley.size().y / 2.0);
        painter.galley(text_pos, galley, text_color);
    }

    /// Draw a text bubble marker for grid lines (for letter labels)
    fn draw_grid_bubble_text(
        &self,
        painter: &egui::Painter,
        pos: egui::Pos2,
        text: &str,
        radius: f32,
        stroke: egui::Stroke,
        text_color: egui::Color32,
    ) {
        // Draw circle
        painter.circle_stroke(pos, radius, stroke);

        // Draw text centered in bubble
        let font_id = egui::FontId::proportional(radius * 1.2);
        let galley = painter.layout_no_wrap(text.to_string(), font_id, text_color);
        let text_pos = egui::pos2(pos.x - galley.size().x / 2.0, pos.y - galley.size().y / 2.0);
        painter.galley(text_pos, galley, text_color);
    }

    /// Render nodes as small dots
    fn render_nodes(&self, painter: &egui::Painter, rect: egui::Rect, view_state: &ViewState) {
        for node in &self.nodes {
            let pos = self.project_to_2d(node.x, node.y, node.z, view_state);
            if rect.contains(pos) {
                painter.circle_filled(pos, ACTIVE_NODE_RADIUS, ACTIVE_NODE_COLOR);
            }
        }
    }

    /// Render elements as lines connecting nodes
    fn render_elements(&self, painter: &egui::Painter, rect: egui::Rect, view_state: &ViewState) {
        for element in &self.elements {
            let node_i = self.nodes.iter().find(|n| n.id == element.node_i_id);
            let node_j = self.nodes.iter().find(|n| n.id == element.node_j_id);

            if let (Some(ni), Some(nj)) = (node_i, node_j) {
                let p1 = self.project_to_2d(ni.x, ni.y, ni.z, view_state);
                let p2 = self.project_to_2d(nj.x, nj.y, nj.z, view_state);

                if rect.contains(p1) || rect.contains(p2) {
                    let color = if element.member_type == "Column" {
                        egui::Color32::from_rgb(200, 50, 50)
                    } else {
                        egui::Color32::from_rgb(50, 150, 50)
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

/// Convert index to letter (0 -> "A", 1 -> "B", ..., 25 -> "Z", 26 -> "AA", ...)
fn index_to_letter(idx: usize) -> String {
    let mut result = String::new();
    let mut n = idx;
    loop {
        result.insert(0, (b'A' + (n % 26) as u8) as char);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::{ViewMode, ViewState};

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

    #[test]
    fn test_project_to_2d_uses_view_state() {
        let mut data = RenderData::new();
        data.add_node(Node {
            id: 1,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        });
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 100.0));
        let mut view_state = ViewState::default();
        view_state.set_view_mode(ViewMode::XY);
        data.calculate_transform(rect, &view_state);
        let point = data.project_to_2d(0.0, 0.0, 0.0, &view_state);
        assert_eq!(point, rect.center());
    }
}
