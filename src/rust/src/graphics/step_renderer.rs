// Step-based cumulative rendering module for AssyPlan
// Provides step-by-step construction visualization with caching for 60fps performance

use eframe::egui::{Color32, Painter, Pos2, Rect, Stroke};
use std::collections::{BTreeSet, HashMap};

use super::{renderer::RenderData, renderer::VisibilitySettings, view_state::ViewState};

/// Step-based render data with cumulative rendering support
///
/// Step N shows elements from Step 1 to N (cumulative)
/// Previous step elements are rendered in grey, current step in color
pub struct StepRenderData {
    /// Base render data containing nodes and elements
    pub base: RenderData,
    /// Element indices grouped by step: step_elements[step-1] = element indices for that step
    pub step_elements: Vec<Vec<usize>>,
    /// Current step being viewed (1-indexed)
    pub current_step: usize,
    /// Maximum step number
    pub max_step: usize,
    /// Cache of cumulative element indices per step for O(1) lookup
    cumulative_cache: HashMap<usize, Vec<usize>>,
    /// Sequence order: element indices in topological/predecessor chain order
    /// sequence_order[i] = element index (into base.elements) for sequence position i
    /// Empty = use base.elements index order (fallback)
    pub sequence_order: Vec<usize>,
}

impl StepRenderData {
    /// Create a new StepRenderData instance with empty step data
    pub fn new(base: RenderData) -> Self {
        Self {
            base,
            step_elements: Vec::new(),
            current_step: 1,
            max_step: 0,
            cumulative_cache: HashMap::new(),
            sequence_order: Vec::new(),
        }
    }

    /// Set sequence order from construction sequence table
    ///
    /// # Arguments
    /// * `sequence_order` - Element indices (into base.elements) in topological/predecessor order
    ///
    /// This defines the order in which elements appear in Sequence mode (1-by-1 playback).
    /// The order is derived from the construction sequence table, which follows predecessor
    /// chain dependencies — giving a mixed Column/Girder order that matches actual assembly order.
    pub fn set_sequence_order(&mut self, sequence_order: Vec<usize>) {
        self.sequence_order = sequence_order;
    }

    /// Set step data from Python step table
    ///
    /// # Arguments
    /// * `step_table` - Vec of (step_number, element_index, member_type) tuples
    ///
    /// Steps are 1-indexed. Elements from Python are 0-indexed.
    pub fn set_step_data(&mut self, step_table: Vec<(i32, usize, String)>) {
        // Clear existing data
        self.step_elements.clear();
        self.invalidate_cache();

        if step_table.is_empty() {
            self.max_step = 0;
            self.current_step = 1;
            return;
        }

        // Find maximum step
        let max_step = step_table
            .iter()
            .map(|(step, _, _)| *step)
            .max()
            .unwrap_or(0) as usize;

        self.max_step = max_step;

        // Initialize step_elements with empty vectors for each step (1-indexed)
        self.step_elements = vec![Vec::new(); max_step.max(1)];

        // Group elements by step
        for (step, element_idx, _member_type) in step_table {
            let step_idx = (step as usize).saturating_sub(1); // Convert to 0-indexed
            if step_idx < self.step_elements.len() {
                self.step_elements[step_idx].push(element_idx);
            }
        }

        // Set current step to 1 (start from beginning)
        if self.max_step > 0 {
            self.current_step = 1;
        }
    }

    /// Get element indices for a specific step (non-cumulative)
    ///
    /// # Arguments
    /// * `step` - Step number (1-indexed)
    ///
    /// # Returns
    /// Slice of element indices for that step, or empty if step is invalid
    pub fn get_elements_for_step(&self, step: usize) -> &[usize] {
        let step_idx = step.saturating_sub(1); // Convert to 0-indexed
        if step_idx < self.step_elements.len() {
            &self.step_elements[step_idx]
        } else {
            &[]
        }
    }

    /// Get cumulative element indices from step 1 to given step (inclusive)
    /// Uses caching for O(1) repeated access - critical for 60fps with 10,000 elements
    ///
    /// # Arguments
    /// * `step` - Step number (1-indexed)
    ///
    /// # Returns
    /// Vec of element indices from steps 1 to step (inclusive)
    pub fn get_cumulative_elements(&mut self, step: usize) -> Vec<usize> {
        // Clamp to valid range
        let step = step.min(self.max_step).max(1);

        // Check cache first
        if let Some(cached) = self.cumulative_cache.get(&step) {
            return cached.clone();
        }

        // Calculate cumulative elements
        let mut cumulative = Vec::new();
        for s in 1..=step {
            let step_idx = s.saturating_sub(1);
            if step_idx < self.step_elements.len() {
                cumulative.extend_from_slice(&self.step_elements[step_idx]);
            }
        }

        // Cache the result
        self.cumulative_cache.insert(step, cumulative.clone());

        cumulative
    }

    /// Set the current step being viewed
    ///
    /// # Arguments
    /// * `step` - Step number (1-indexed)
    ///
    /// # Returns
    /// true if step was valid and set, false otherwise
    pub fn set_current_step(&mut self, step: usize) -> bool {
        if step == 0 || step > self.max_step {
            return false;
        }
        self.current_step = step;
        true
    }

    /// Move to the next step
    ///
    /// # Returns
    /// true if successful, false if already at max step
    pub fn next_step(&mut self) -> bool {
        if self.current_step >= self.max_step {
            return false;
        }
        self.current_step += 1;
        true
    }

    /// Move to the previous step
    ///
    /// # Returns
    /// true if successful, false if already at step 1
    pub fn prev_step(&mut self) -> bool {
        if self.current_step <= 1 {
            return false;
        }
        self.current_step -= 1;
        true
    }

    /// Invalidate the cumulative cache - call when step_elements changes
    pub fn invalidate_cache(&mut self) {
        self.cumulative_cache.clear();
    }

    /// Get the color for an element based on its step
    ///
    /// # Arguments
    /// * `element_idx` - Index of the element in the elements array
    /// * `is_current_step` - Whether this element belongs to the current step
    ///
    /// # Returns
    /// Color32 for rendering
    fn get_element_color(&self, element_idx: usize, is_current_step: bool) -> Color32 {
        // Get the element to determine its type
        if element_idx >= self.base.elements.len() {
            return Color32::GRAY;
        }

        let element = &self.base.elements[element_idx];

        if is_current_step {
            // Current step: use original colors
            if element.member_type == "Column" {
                Color32::from_rgb(200, 50, 50) // Red for columns
            } else {
                Color32::from_rgb(50, 150, 50) // Green for girders
            }
        } else {
            // Previous steps: render in grey with transparency
            Color32::from_gray(150)
        }
    }

    /// Check if an element belongs to a specific step
    #[allow(dead_code)]
    fn is_element_in_step(&self, element_idx: usize, step: usize) -> bool {
        let step_idx = step.saturating_sub(1);
        if step_idx < self.step_elements.len() {
            self.step_elements[step_idx].contains(&element_idx)
        } else {
            false
        }
    }

    /// Render with step-based coloring
    ///
    /// - Steps 1 to (current_step-1): grey, semi-transparent
    /// - Step current_step: original colors (red for Column, green for Girder)
    /// - Grid and nodes: render based on visibility settings
    ///
    /// # Arguments
    /// * `painter` - Egui painter for rendering
    /// * `rect` - Clipping rectangle
    /// * `visibility` - Visibility settings for grid, nodes, elements
    pub fn render_step(
        &self,
        painter: &Painter,
        rect: Rect,
        view_state: &ViewState,
        visibility: &VisibilitySettings,
    ) {
        // Render grid and nodes based on visibility settings
        if visibility.show_grid {
            self.render_grid_inline(painter, rect, view_state);
        }
        if visibility.show_nodes {
            self.render_nodes_inline(painter, rect, view_state);
        }

        // Render elements with step coloring
        if visibility.show_elements {
            self.render_step_elements(painter, rect, view_state);
        }
    }

    /// Render with sequence-based display (elements 1 to N in original input order)
    ///
    /// - Elements 1 to (current_sequence-1): grey, semi-transparent
    /// - Element current_sequence: original colors (red for Column, green for Girder)
    /// - Grid and nodes: render based on visibility settings
    ///
    /// # Arguments
    /// * `painter` - Egui painter for rendering
    /// * `rect` - Clipping rectangle
    /// * `view_state` - Current view state for projection
    /// * `visibility` - Visibility settings for grid, nodes, elements
    /// * `current_sequence` - Current sequence number (1-indexed, 1 to max_sequence)
    pub fn render_sequence(
        &self,
        painter: &Painter,
        rect: Rect,
        view_state: &ViewState,
        visibility: &VisibilitySettings,
        current_sequence: usize,
    ) {
        // Render grid and nodes based on visibility settings
        if visibility.show_grid {
            self.render_grid_inline(painter, rect, view_state);
        }
        if visibility.show_nodes {
            self.render_nodes_inline(painter, rect, view_state);
        }

        // Render elements up to current_sequence
        if visibility.show_elements {
            self.render_sequence_elements(painter, rect, view_state, current_sequence);
        }
    }

    /// Render elements in sequence order (construction sequence / predecessor chain order)
    ///
    /// Uses `sequence_order` (topological order from predecessor chain) if available,
    /// falling back to raw element index order. Sequence order gives a mixed Column/Girder
    /// ordering that matches actual construction assembly sequence.
    fn render_sequence_elements(
        &self,
        painter: &Painter,
        rect: Rect,
        view_state: &ViewState,
        current_sequence: usize,
    ) {
        if self.base.elements.is_empty() || current_sequence == 0 {
            return;
        }

        // Use sequence_order if populated (topological order), else fallback to index order
        let total = if !self.sequence_order.is_empty() {
            self.sequence_order.len()
        } else {
            self.base.elements.len()
        };

        // current_sequence is 1-indexed; render positions 0 to (current_sequence-1)
        let max_pos = current_sequence.min(total);

        for pos in 0..max_pos {
            // Resolve element index: use sequence_order if available
            let elem_idx = if !self.sequence_order.is_empty() {
                self.sequence_order[pos]
            } else {
                pos
            };

            if elem_idx >= self.base.elements.len() {
                continue;
            }

            let element = &self.base.elements[elem_idx];

            // Determine color: current element (last in this render) is highlighted, others grey
            let is_current = pos == max_pos - 1;
            let color = if is_current {
                if element.member_type == "Column" {
                    Color32::from_rgb(200, 50, 50) // Red for columns
                } else {
                    Color32::from_rgb(50, 150, 50) // Green for girders
                }
            } else {
                Color32::from_gray(150)
            };

            // Find the start and end nodes
            let node_i = self.base.nodes.iter().find(|n| n.id == element.node_i_id);
            let node_j = self.base.nodes.iter().find(|n| n.id == element.node_j_id);

            if let (Some(ni), Some(nj)) = (node_i, node_j) {
                let p1 = self.base.project_to_2d(ni.x, ni.y, ni.z, view_state);
                let p2 = self.base.project_to_2d(nj.x, nj.y, nj.z, view_state);

                if rect.contains(p1) || rect.contains(p2) {
                    let stroke_width = if is_current { 2.0 } else { 1.5 };
                    painter.line_segment([p1, p2], Stroke::new(stroke_width, color));
                }
            }
        }
    }

    /// Render grid lines (Revit-style with bubble markers)
    /// Grid lines are drawn at unique X and Y node coordinates on the min_z plane
    fn render_grid_inline(&self, painter: &Painter, _rect: Rect, view_state: &ViewState) {
        if self.base.nodes.is_empty() {
            return;
        }

        // Collect unique X and Y coordinates using BTreeSet for sorted order
        // Use i64 keys (coordinate * 10) to handle floating point comparison
        let unique_x: BTreeSet<i64> = self
            .base
            .nodes
            .iter()
            .map(|n| (n.x * 10.0).round() as i64)
            .collect();
        let unique_y: BTreeSet<i64> = self
            .base
            .nodes
            .iter()
            .map(|n| (n.y * 10.0).round() as i64)
            .collect();

        // Find bounds
        let min_x = self
            .base
            .nodes
            .iter()
            .map(|n| n.x)
            .fold(f64::INFINITY, f64::min);
        let max_x = self
            .base
            .nodes
            .iter()
            .map(|n| n.x)
            .fold(f64::NEG_INFINITY, f64::max);
        let min_y = self
            .base
            .nodes
            .iter()
            .map(|n| n.y)
            .fold(f64::INFINITY, f64::min);
        let max_y = self
            .base
            .nodes
            .iter()
            .map(|n| n.y)
            .fold(f64::NEG_INFINITY, f64::max);
        let min_z = self
            .base
            .nodes
            .iter()
            .map(|n| n.z)
            .fold(f64::INFINITY, f64::min);

        // Grid line stroke
        let stroke = Stroke::new(1.0, Color32::from_gray(120));

        // Extension distance for grid lines beyond data bounds (for bubble marker placement)
        let extend_dist = (max_x - min_x).max(max_y - min_y) * 0.15;

        // Bubble marker settings
        let bubble_radius = 12.0_f32;
        let bubble_stroke = Stroke::new(1.5, Color32::from_gray(100));
        let text_color = Color32::from_gray(80);

        // Draw X-direction grid lines (vertical lines in plan view)
        for (idx, &x_key) in unique_x.iter().enumerate() {
            let x = x_key as f64 / 10.0;
            let p1 = self
                .base
                .project_to_2d(x, min_y - extend_dist, min_z, view_state);
            let p2 = self
                .base
                .project_to_2d(x, max_y + extend_dist, min_z, view_state);

            // Trim line start so it doesn't intrude into the bubble circle
            let dir = (p2 - p1).normalized();
            let p1_trimmed = p1 + dir * bubble_radius;

            // Draw grid line (starting from trimmed point to avoid bubble overlap)
            painter.line_segment([p1_trimmed, p2], stroke);

            // Draw numbered bubble marker at extended end
            self.draw_grid_bubble(
                painter,
                p1,
                idx + 1,
                bubble_radius,
                bubble_stroke,
                text_color,
            );
        }

        // Draw Y-direction grid lines (horizontal lines in plan view)
        // Use letters A, B, C... for Y-direction grids (Revit convention)
        for (idx, &y_key) in unique_y.iter().enumerate() {
            let y = y_key as f64 / 10.0;
            let p1 = self
                .base
                .project_to_2d(min_x - extend_dist, y, min_z, view_state);
            let p2 = self
                .base
                .project_to_2d(max_x + extend_dist, y, min_z, view_state);

            // Trim line start so it doesn't intrude into the bubble circle
            let dir = (p2 - p1).normalized();
            let p1_trimmed = p1 + dir * bubble_radius;

            // Draw grid line (starting from trimmed point to avoid bubble overlap)
            painter.line_segment([p1_trimmed, p2], stroke);

            // Draw lettered bubble marker at extended end
            let label = Self::index_to_letter(idx);
            self.draw_grid_bubble_text(
                painter,
                p1,
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
        painter: &Painter,
        pos: Pos2,
        number: usize,
        radius: f32,
        stroke: Stroke,
        text_color: Color32,
    ) {
        // Draw circle
        painter.circle_stroke(pos, radius, stroke);

        // Draw number text centered in bubble
        let text = format!("{}", number);
        let font_id = eframe::egui::FontId::proportional(radius * 1.2);
        let galley = painter.layout_no_wrap(text, font_id, text_color);
        let text_pos = Pos2::new(pos.x - galley.size().x / 2.0, pos.y - galley.size().y / 2.0);
        painter.galley(text_pos, galley, text_color);
    }

    /// Draw a text bubble marker for grid lines (for letter labels)
    fn draw_grid_bubble_text(
        &self,
        painter: &Painter,
        pos: Pos2,
        text: &str,
        radius: f32,
        stroke: Stroke,
        text_color: Color32,
    ) {
        // Draw circle
        painter.circle_stroke(pos, radius, stroke);

        // Draw text centered in bubble
        let font_id = eframe::egui::FontId::proportional(radius * 1.2);
        let galley = painter.layout_no_wrap(text.to_string(), font_id, text_color);
        let text_pos = Pos2::new(pos.x - galley.size().x / 2.0, pos.y - galley.size().y / 2.0);
        painter.galley(text_pos, galley, text_color);
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

    /// Render nodes as small dots (inlined from RenderData to avoid private method issue)
    fn render_nodes_inline(&self, painter: &Painter, rect: Rect, view_state: &ViewState) {
        for node in &self.base.nodes {
            let pos = self.base.project_to_2d(node.x, node.y, node.z, view_state);
            if rect.contains(pos) {
                // Draw a small circle for each node
                let radius = 3.0;
                painter.circle_filled(pos, radius, Color32::from_rgb(0, 100, 200));
            }
        }
    }

    /// Render elements with step-based coloring
    fn render_step_elements(&self, painter: &Painter, rect: Rect, view_state: &ViewState) {
        if self.base.elements.is_empty() || self.max_step == 0 {
            return;
        }

        // Get cumulative elements up to current_step
        let current = self.current_step;

        for (element_idx, element) in self.base.elements.iter().enumerate() {
            // Find which step this element belongs to
            let mut element_step = 0;
            for (step_idx, step_elements) in self.step_elements.iter().enumerate() {
                if step_elements.contains(&element_idx) {
                    element_step = step_idx + 1; // Convert to 1-indexed
                    break;
                }
            }

            // Skip elements not yet constructed
            if element_step == 0 || element_step > current {
                continue;
            }

            // Determine color based on step
            let is_current_step = element_step == current;
            let color = self.get_element_color(element_idx, is_current_step);

            // Find the start and end nodes
            let node_i = self.base.nodes.iter().find(|n| n.id == element.node_i_id);
            let node_j = self.base.nodes.iter().find(|n| n.id == element.node_j_id);

            if let (Some(ni), Some(nj)) = (node_i, node_j) {
                let p1 = self.base.project_to_2d(ni.x, ni.y, ni.z, view_state);
                let p2 = self.base.project_to_2d(nj.x, nj.y, nj.z, view_state);

                // Only draw if at least one point is in rect
                if rect.contains(p1) || rect.contains(p2) {
                    let stroke_width = if is_current_step { 2.0 } else { 1.5 };
                    painter.line_segment([p1, p2], Stroke::new(stroke_width, color));
                }
            }
        }
    }

    /// Get step info for display
    pub fn get_step_info(&self) -> String {
        if self.max_step == 0 {
            return "No steps".to_string();
        }
        format!("Step {}/{}", self.current_step, self.max_step)
    }

    /// Check if there are steps to display
    pub fn has_steps(&self) -> bool {
        self.max_step > 0
    }

    /// Get total element count across all steps
    pub fn total_element_count(&self) -> usize {
        self.step_elements.iter().map(|v| v.len()).sum()
    }

    /// Get element count for a specific step
    pub fn get_step_element_count(&self, step: usize) -> usize {
        self.get_elements_for_step(step).len()
    }
}

impl Default for StepRenderData {
    fn default() -> Self {
        Self::new(RenderData::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::renderer;

    fn create_test_render_data() -> RenderData {
        let mut data = RenderData::new();

        // Add nodes (id, x, y, z)
        data.add_node(renderer::Node {
            id: 1,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        });
        data.add_node(renderer::Node {
            id: 2,
            x: 1000.0,
            y: 0.0,
            z: 0.0,
        });
        data.add_node(renderer::Node {
            id: 3,
            x: 0.0,
            y: 1000.0,
            z: 0.0,
        });
        data.add_node(renderer::Node {
            id: 4,
            x: 1000.0,
            y: 1000.0,
            z: 0.0,
        });

        // Add elements (id, node_i_id, node_j_id, member_type)
        data.add_element(renderer::Element {
            id: 1,
            node_i_id: 1,
            node_j_id: 2,
            member_type: "Column".to_string(),
        });
        data.add_element(renderer::Element {
            id: 2,
            node_i_id: 2,
            node_j_id: 4,
            member_type: "Girder".to_string(),
        });
        data.add_element(renderer::Element {
            id: 3,
            node_i_id: 1,
            node_j_id: 3,
            member_type: "Column".to_string(),
        });
        data.add_element(renderer::Element {
            id: 4,
            node_i_id: 3,
            node_j_id: 4,
            member_type: "Girder".to_string(),
        });

        data
    }

    #[test]
    fn test_step_render_data_new() {
        let base = create_test_render_data();
        let step_data = StepRenderData::new(base);

        assert_eq!(step_data.current_step, 1);
        assert_eq!(step_data.max_step, 0);
        assert!(step_data.step_elements.is_empty());
    }

    #[test]
    fn test_set_step_data() {
        let base = create_test_render_data();
        let mut step_data = StepRenderData::new(base);

        // step_table: (step_number, element_index, member_type)
        let step_table = vec![
            (1, 0, "Column".to_string()),
            (1, 1, "Girder".to_string()),
            (2, 2, "Column".to_string()),
            (2, 3, "Girder".to_string()),
        ];

        step_data.set_step_data(step_table);

        assert_eq!(step_data.max_step, 2);
        assert_eq!(step_data.current_step, 1); // Starts at step 1
        assert_eq!(step_data.step_elements.len(), 2);
        assert_eq!(step_data.get_elements_for_step(1), vec![0, 1]);
        assert_eq!(step_data.get_elements_for_step(2), vec![2, 3]);
    }

    #[test]
    fn test_cumulative_elements() {
        let base = create_test_render_data();
        let mut step_data = StepRenderData::new(base);

        let step_table = vec![
            (1, 0, "Column".to_string()),
            (1, 1, "Girder".to_string()),
            (2, 2, "Column".to_string()),
            (2, 3, "Girder".to_string()),
        ];

        step_data.set_step_data(step_table);

        // Step 1 cumulative: [0, 1]
        let cum1 = step_data.get_cumulative_elements(1);
        assert_eq!(cum1, vec![0, 1]);

        // Step 2 cumulative: [0, 1, 2, 3]
        let cum2 = step_data.get_cumulative_elements(2);
        assert_eq!(cum2, vec![0, 1, 2, 3]);
    }

    #[test]
    fn test_cumulative_cache() {
        let base = create_test_render_data();
        let mut step_data = StepRenderData::new(base);

        let step_table = vec![
            (1, 0, "Column".to_string()),
            (2, 1, "Girder".to_string()),
            (3, 2, "Column".to_string()),
        ];

        step_data.set_step_data(step_table);

        // First call computes and caches
        let _ = step_data.get_cumulative_elements(2);
        assert_eq!(step_data.cumulative_cache.len(), 1);

        // Second call uses cache
        let _ = step_data.get_cumulative_elements(2);
        assert_eq!(step_data.cumulative_cache.len(), 1);
    }

    #[test]
    fn test_cache_invalidation() {
        let base = create_test_render_data();
        let mut step_data = StepRenderData::new(base);

        let step_table = vec![(1, 0, "Column".to_string()), (2, 1, "Girder".to_string())];

        step_data.set_step_data(step_table);
        let _ = step_data.get_cumulative_elements(2);
        assert!(!step_data.cumulative_cache.is_empty());

        // Invalidate and verify cache is cleared
        step_data.invalidate_cache();
        assert!(step_data.cumulative_cache.is_empty());
    }

    #[test]
    fn test_set_current_step() {
        let base = create_test_render_data();
        let mut step_data = StepRenderData::new(base);

        let step_table = vec![
            (1, 0, "Column".to_string()),
            (2, 1, "Girder".to_string()),
            (3, 2, "Column".to_string()),
        ];

        step_data.set_step_data(step_table);

        // Valid step
        assert!(step_data.set_current_step(2));
        assert_eq!(step_data.current_step, 2);

        // Invalid: step 0
        assert!(!step_data.set_current_step(0));

        // Invalid: step > max
        assert!(!step_data.set_current_step(10));
    }

    #[test]
    fn test_next_step() {
        let base = create_test_render_data();
        let mut step_data = StepRenderData::new(base);

        let step_table = vec![(1, 0, "Column".to_string()), (2, 1, "Girder".to_string())];

        step_data.set_step_data(step_table);

        // Can go next from step 1
        assert!(step_data.next_step());
        assert_eq!(step_data.current_step, 2);

        // Cannot go next from max step
        assert!(!step_data.next_step());
    }

    #[test]
    fn test_prev_step() {
        let base = create_test_render_data();
        let mut step_data = StepRenderData::new(base);

        let step_table = vec![(1, 0, "Column".to_string()), (2, 1, "Girder".to_string())];

        step_data.set_step_data(step_table);

        // Start at step 1
        assert_eq!(step_data.current_step, 1);

        // Can go next to step 2
        assert!(step_data.next_step());
        assert_eq!(step_data.current_step, 2);

        // Can go prev from step 2
        assert!(step_data.prev_step());
        assert_eq!(step_data.current_step, 1);

        // Cannot go prev from step 1
        assert!(!step_data.prev_step());
    }

    #[test]
    fn test_boundary_conditions() {
        let base = create_test_render_data();
        let mut step_data = StepRenderData::new(base);

        // Empty step table
        step_data.set_step_data(vec![]);
        assert_eq!(step_data.max_step, 0);
        assert_eq!(step_data.current_step, 1);

        // Step 0 should return empty
        assert!(step_data.get_elements_for_step(0).is_empty());

        // Step beyond max should return empty
        assert!(step_data.get_elements_for_step(100).is_empty());

        // Cumulative with step 0 returns empty
        let cum = step_data.get_cumulative_elements(0);
        assert!(cum.is_empty());

        // Cumulative with step > max clamps to max
        let step_table = vec![(1, 0, "Column".to_string())];
        step_data.set_step_data(step_table);
        let cum = step_data.get_cumulative_elements(100);
        assert_eq!(cum, vec![0]);
    }

    #[test]
    fn test_step_info() {
        let base = create_test_render_data();
        let mut step_data = StepRenderData::new(base);

        // No steps
        assert_eq!(step_data.get_step_info(), "No steps");

        // With steps - starts at step 1
        let step_table = vec![(1, 0, "Column".to_string()), (2, 1, "Girder".to_string())];
        step_data.set_step_data(step_table);

        assert_eq!(step_data.get_step_info(), "Step 1/2");
    }

    #[test]
    fn test_has_steps() {
        let base = create_test_render_data();
        let mut step_data = StepRenderData::new(base);

        assert!(!step_data.has_steps());

        let step_table = vec![(1, 0, "Column".to_string())];
        step_data.set_step_data(step_table);

        assert!(step_data.has_steps());
    }

    #[test]
    fn test_element_counts() {
        let base = create_test_render_data();
        let mut step_data = StepRenderData::new(base);

        let step_table = vec![
            (1, 0, "Column".to_string()),
            (1, 1, "Girder".to_string()),
            (2, 2, "Column".to_string()),
            (2, 3, "Girder".to_string()),
            (3, 0, "Column".to_string()), // Duplicate element in different step
        ];

        step_data.set_step_data(step_table);

        assert_eq!(step_data.total_element_count(), 5);
        assert_eq!(step_data.get_step_element_count(1), 2);
        assert_eq!(step_data.get_step_element_count(2), 2);
        assert_eq!(step_data.get_step_element_count(3), 1);
    }

    #[test]
    fn test_single_step() {
        let base = create_test_render_data();
        let mut step_data = StepRenderData::new(base);

        let step_table = vec![
            (1, 0, "Column".to_string()),
            (1, 1, "Girder".to_string()),
            (1, 2, "Column".to_string()),
        ];

        step_data.set_step_data(step_table);

        assert_eq!(step_data.max_step, 1);
        assert_eq!(step_data.current_step, 1);

        // All elements in step 1
        let cum1 = step_data.get_cumulative_elements(1);
        assert_eq!(cum1, vec![0, 1, 2]);

        // Can't navigate with single step
        assert!(!step_data.next_step());
        assert!(!step_data.prev_step());
    }

    // ========================================================================
    // Integration Tests: Step Rendering + UI State + Cache Coordination
    // ========================================================================

    #[test]
    fn test_step_data_ui_state_sync() {
        // Tests that step data and UI state sync correctly
        let base = create_test_render_data();
        let mut step_data = StepRenderData::new(base);

        let step_table = vec![
            (1, 0, "Column".to_string()),
            (2, 1, "Girder".to_string()),
            (3, 2, "Column".to_string()),
        ];

        step_data.set_step_data(step_table);

        // Initial state should be step 1
        assert_eq!(step_data.current_step, 1);
        assert_eq!(step_data.max_step, 3);

        // Navigate forward
        assert!(step_data.next_step());
        assert_eq!(step_data.current_step, 2);

        assert!(step_data.next_step());
        assert_eq!(step_data.current_step, 3);

        // At max, can't go further
        assert!(!step_data.next_step());
        assert_eq!(step_data.current_step, 3);

        // Navigate backward
        assert!(step_data.prev_step());
        assert_eq!(step_data.current_step, 2);

        assert!(step_data.prev_step());
        assert_eq!(step_data.current_step, 1);

        // At min, can't go further
        assert!(!step_data.prev_step());
        assert_eq!(step_data.current_step, 1);
    }

    #[test]
    fn test_cumulative_cache_with_navigation() {
        // Tests that cache works correctly during navigation
        let base = create_test_render_data();
        let mut step_data = StepRenderData::new(base);

        let step_table = vec![
            (1, 0, "Column".to_string()),
            (2, 1, "Girder".to_string()),
            (3, 2, "Column".to_string()),
        ];

        step_data.set_step_data(step_table);

        // Navigate to step 2 and get cumulative
        step_data.set_current_step(2);
        let cum2_first = step_data.get_cumulative_elements(2);
        assert_eq!(cum2_first, vec![0, 1]);

        // Cache should be populated
        assert_eq!(step_data.cumulative_cache.len(), 1);

        // Navigate to step 3
        step_data.next_step();
        let cum3 = step_data.get_cumulative_elements(3);
        assert_eq!(cum3, vec![0, 1, 2]);

        // Cache should have both entries
        assert_eq!(step_data.cumulative_cache.len(), 2);

        // Navigate back to step 2 - should use cache
        step_data.prev_step();
        let cum2_second = step_data.get_cumulative_elements(2);
        assert_eq!(cum2_second, cum2_first);

        // Cache size unchanged (reused)
        assert_eq!(step_data.cumulative_cache.len(), 2);
    }

    #[test]
    fn test_step_data_reset_invalidates_cache() {
        // Tests that setting new step data invalidates cache
        let base = create_test_render_data();
        let mut step_data = StepRenderData::new(base);

        // Initial step data
        let step_table1 = vec![(1, 0, "Column".to_string()), (2, 1, "Girder".to_string())];

        step_data.set_step_data(step_table1);
        let _ = step_data.get_cumulative_elements(2);
        assert!(!step_data.cumulative_cache.is_empty());

        // Set new step data - cache should be cleared
        let step_table2 = vec![
            (1, 0, "Column".to_string()),
            (1, 1, "Girder".to_string()),
            (2, 2, "Column".to_string()),
        ];

        step_data.set_step_data(step_table2);
        assert!(step_data.cumulative_cache.is_empty());
    }

    #[test]
    fn test_direct_step_input_validation() {
        // Tests direct step input (simulating UI input)
        let base = create_test_render_data();
        let mut step_data = StepRenderData::new(base);

        let step_table = vec![
            (1, 0, "Column".to_string()),
            (2, 1, "Girder".to_string()),
            (3, 2, "Column".to_string()),
            (4, 3, "Girder".to_string()),
            (5, 4, "Column".to_string()),
        ];

        step_data.set_step_data(step_table);

        // Valid direct input
        assert!(step_data.set_current_step(3));
        assert_eq!(step_data.current_step, 3);

        // Jump to max
        assert!(step_data.set_current_step(5));
        assert_eq!(step_data.current_step, 5);

        // Jump to min
        assert!(step_data.set_current_step(1));
        assert_eq!(step_data.current_step, 1);

        // Invalid: step 0
        assert!(!step_data.set_current_step(0));
        assert_eq!(step_data.current_step, 1); // unchanged

        // Invalid: step > max
        assert!(!step_data.set_current_step(100));
        assert_eq!(step_data.current_step, 1); // unchanged
    }

    #[test]
    fn test_many_steps_performance_scenario() {
        // Tests handling of many steps (simulates large model)
        let mut base = RenderData::new();

        // Create 100 nodes
        for i in 1..=100 {
            base.add_node(renderer::Node {
                id: i,
                x: (i as f64) * 1000.0,
                y: 0.0,
                z: 0.0,
            });
        }

        // Create 99 elements connecting consecutive nodes
        for i in 0..99 {
            base.add_element(renderer::Element {
                id: (i + 1) as i32,
                node_i_id: (i + 1) as i32,
                node_j_id: (i + 2) as i32,
                member_type: if i % 2 == 0 {
                    "Column".to_string()
                } else {
                    "Girder".to_string()
                },
            });
        }

        let mut step_data = StepRenderData::new(base);

        // Create step table with 50 steps (2 elements per step)
        let step_table: Vec<(i32, usize, String)> = (0..99)
            .map(|i| {
                let step = (i / 2) + 1;
                let member_type = if i % 2 == 0 { "Column" } else { "Girder" };
                (step as i32, i, member_type.to_string())
            })
            .collect();

        step_data.set_step_data(step_table);

        assert_eq!(step_data.max_step, 50);
        assert_eq!(step_data.total_element_count(), 99);

        // Navigate to middle step
        assert!(step_data.set_current_step(25));
        let cum25 = step_data.get_cumulative_elements(25);
        assert_eq!(cum25.len(), 50); // 25 steps * 2 elements per step

        // Navigate to last step
        assert!(step_data.set_current_step(50));
        let cum50 = step_data.get_cumulative_elements(50);
        assert_eq!(cum50.len(), 99); // All elements

        // Verify cache is working
        assert!(!step_data.cumulative_cache.is_empty());
    }

    #[test]
    fn test_element_visibility_by_step() {
        // Tests that elements are correctly visible/hidden by step
        let base = create_test_render_data();
        let mut step_data = StepRenderData::new(base);

        let step_table = vec![
            (1, 0, "Column".to_string()), // Element 0 in step 1
            (1, 1, "Girder".to_string()), // Element 1 in step 1
            (2, 2, "Column".to_string()), // Element 2 in step 2
            (3, 3, "Girder".to_string()), // Element 3 in step 3
        ];

        step_data.set_step_data(step_table);

        // At step 1: only elements 0, 1 visible
        step_data.set_current_step(1);
        let step1_elements = step_data.get_cumulative_elements(1);
        assert_eq!(step1_elements, vec![0, 1]);
        assert_eq!(step_data.get_step_element_count(1), 2);

        // At step 2: elements 0, 1, 2 visible
        step_data.set_current_step(2);
        let step2_elements = step_data.get_cumulative_elements(2);
        assert_eq!(step2_elements, vec![0, 1, 2]);
        assert_eq!(step_data.get_step_element_count(2), 1);

        // At step 3: all elements visible
        step_data.set_current_step(3);
        let step3_elements = step_data.get_cumulative_elements(3);
        assert_eq!(step3_elements, vec![0, 1, 2, 3]);
        assert_eq!(step_data.get_step_element_count(3), 1);
    }

    #[test]
    fn test_get_step_info_formats() {
        // Tests step info formatting
        let base = create_test_render_data();
        let mut step_data = StepRenderData::new(base);

        // No steps
        assert_eq!(step_data.get_step_info(), "No steps");

        // With steps
        let step_table = vec![
            (1, 0, "Column".to_string()),
            (2, 1, "Girder".to_string()),
            (3, 2, "Column".to_string()),
        ];
        step_data.set_step_data(step_table);

        // At step 1
        assert_eq!(step_data.get_step_info(), "Step 1/3");

        // Navigate to step 2
        step_data.next_step();
        assert_eq!(step_data.get_step_info(), "Step 2/3");

        // Navigate to step 3
        step_data.next_step();
        assert_eq!(step_data.get_step_info(), "Step 3/3");
    }
}
