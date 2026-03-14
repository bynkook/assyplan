// Step-based cumulative rendering module for AssyPlan
// Provides step-by-step construction visualization with caching for 60fps performance

use eframe::egui::{Color32, Painter, Rect, Stroke};
use std::collections::HashMap;

use super::renderer::RenderData;

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
        }
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
    /// - Grid and nodes: always render normally
    ///
    /// # Arguments
    /// * `painter` - Egui painter for rendering
    /// * `rect` - Clipping rectangle
    pub fn render_step(&self, painter: &Painter, rect: Rect) {
        // Render grid and nodes always (all visible) - inline to avoid private method issue
        self.render_grid_inline(painter, rect);
        self.render_nodes_inline(painter, rect);

        // Render elements with step coloring
        self.render_step_elements(painter, rect);
    }

    /// Render grid lines (inlined from RenderData to avoid private method issue)
    fn render_grid_inline(&self, painter: &Painter, rect: Rect) {
        if self.base.nodes.is_empty() {
            return;
        }

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

        let grid_size = 1000.0; // Grid spacing

        // Vertical grid lines
        let start_x = (min_x / grid_size).floor() * grid_size;
        let end_x = (max_x / grid_size).ceil() * grid_size;
        let mut x = start_x;
        while x <= end_x {
            let p1 = self.base.project_to_2d(x, min_y, 0.0);
            let p2 = self.base.project_to_2d(x, max_y, 0.0);
            if rect.contains(p1) || rect.contains(p2) {
                painter.line_segment([p1, p2], Stroke::new(1.0, Color32::from_gray(100)));
            }
            x += grid_size;
        }

        // Horizontal grid lines
        let start_y = (min_y / grid_size).floor() * grid_size;
        let end_y = (max_y / grid_size).ceil() * grid_size;
        let mut y = start_y;
        while y <= end_y {
            let p1 = self.base.project_to_2d(min_x, y, 0.0);
            let p2 = self.base.project_to_2d(max_x, y, 0.0);
            if rect.contains(p1) || rect.contains(p2) {
                painter.line_segment([p1, p2], Stroke::new(1.0, Color32::from_gray(100)));
            }
            y += grid_size;
        }
    }

    /// Render nodes as small dots (inlined from RenderData to avoid private method issue)
    fn render_nodes_inline(&self, painter: &Painter, rect: Rect) {
        for node in &self.base.nodes {
            let pos = self.base.project_to_2d(node.x, node.y, node.z);
            if rect.contains(pos) {
                // Draw a small circle for each node
                let radius = 3.0;
                painter.circle_filled(pos, radius, Color32::from_rgb(0, 100, 200));
            }
        }
    }

    /// Render elements with step-based coloring
    fn render_step_elements(&self, painter: &Painter, rect: Rect) {
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
                let p1 = self.base.project_to_2d(ni.x, ni.y, ni.z);
                let p2 = self.base.project_to_2d(nj.x, nj.y, nj.z);

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

// Import the parent module for RenderData
use super::renderer;

#[cfg(test)]
mod tests {
    use super::*;

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
