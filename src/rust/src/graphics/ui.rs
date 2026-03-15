// UI module for AssyPlan application

use eframe::egui::{self, Widget};

/// View mode for the graphics display
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DisplayMode {
    /// Show all elements (full model)
    Model,
    /// Show elements step-by-step (construction sequence)
    Construction,
}

impl Default for DisplayMode {
    fn default() -> Self {
        DisplayMode::Model
    }
}

/// Construction view mode - Sequence (individual) vs Step (stability groups)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConstructionViewMode {
    /// Show elements one by one in installation order (1, 2, 3, ... 259)
    Sequence,
    /// Show elements grouped by stability step (Step 1, Step 2, ...)
    Step,
}

impl Default for ConstructionViewMode {
    fn default() -> Self {
        ConstructionViewMode::Sequence
    }
}

/// UI State for managing application state
pub struct UiState {
    /// Whether to show ID labels on nodes
    pub show_id_labels: bool,
    /// Whether to show node IDs
    pub show_node_ids: bool,
    /// Whether to show element IDs
    pub show_element_ids: bool,
    /// Current mode: "Development" or "Simulation"
    pub mode: String,
    /// Current file path loaded
    pub file_path: String,
    /// Current tab selection
    pub current_tab: String,
    /// Status message to display
    pub status_message: String,
    /// Whether data has been loaded
    pub has_data: bool,
    /// Whether validation passed
    pub validation_passed: bool,
    /// Current step (1-indexed)
    pub current_step: usize,
    /// Maximum step number
    pub max_step: usize,
    /// Direct input for step number
    pub step_input: String,
    /// Visibility: show grid lines
    pub show_grid: bool,
    /// Visibility: show nodes
    pub show_nodes: bool,
    /// Visibility: show elements
    pub show_elements: bool,
    /// Display mode: Model (full) or Construction (step-by-step)
    pub display_mode: DisplayMode,
    /// Whether step data has been calculated
    pub has_step_data: bool,
    /// Construction view mode: Sequence (individual) or Step (groups)
    pub construction_view_mode: ConstructionViewMode,
    /// Current sequence index (1-indexed, for Sequence mode)
    pub current_sequence: usize,
    /// Maximum sequence number (total elements)
    pub max_sequence: usize,
    // ============================================================================
    // Construction Metrics (populated after table generation)
    // ============================================================================
    /// Total element count
    pub total_elements: usize,
    /// Total column count
    pub total_columns: usize,
    /// Total girder count
    pub total_girders: usize,
    /// Number of workfronts
    pub workfront_count: usize,
    /// Floor column counts: (floor_level, total_columns, installed_columns)
    pub floor_column_data: Vec<(i32, usize, usize)>,
    /// Whether recalculation is needed (after load or reset)
    pub needs_recalc: bool,
    /// Elements installed at each step: step -> Vec<(element_id, member_type, floor)>
    /// Used to calculate step-based metrics
    pub step_elements: Vec<Vec<(i32, String, i32)>>,
}

impl UiState {
    /// Create a new UiState instance
    pub fn new() -> Self {
        Self {
            show_id_labels: true,
            show_node_ids: true,
            show_element_ids: true,
            mode: "Development".to_string(),
            file_path: String::new(),
            current_tab: "View".to_string(),
            status_message: "Ready".to_string(),
            has_data: false,
            validation_passed: false,
            current_step: 1,
            max_step: 0,
            step_input: "1".to_string(),
            show_grid: true,
            show_nodes: true,
            show_elements: true,
            display_mode: DisplayMode::Model,
            has_step_data: false,
            construction_view_mode: ConstructionViewMode::Sequence,
            current_sequence: 1,
            max_sequence: 0,
            // Metrics - initialized empty
            total_elements: 0,
            total_columns: 0,
            total_girders: 0,
            workfront_count: 0,
            floor_column_data: Vec::new(),
            needs_recalc: false,
            step_elements: Vec::new(),
        }
    }

    /// Reset state to initial values
    pub fn reset(&mut self) {
        self.show_id_labels = true;
        self.show_node_ids = true;
        self.show_element_ids = true;
        self.mode = "Development".to_string();
        self.file_path.clear();
        self.current_tab = "View".to_string();
        self.status_message = "Ready".to_string();
        self.has_data = false;
        self.validation_passed = false;
        self.current_step = 1;
        self.max_step = 0;
        self.step_input = "1".to_string();
        self.show_grid = true;
        self.show_nodes = true;
        self.show_elements = true;
        self.display_mode = DisplayMode::Model;
        self.has_step_data = false;
        self.construction_view_mode = ConstructionViewMode::Sequence;
        self.current_sequence = 1;
        self.max_sequence = 0;
        // Reset metrics
        self.total_elements = 0;
        self.total_columns = 0;
        self.total_girders = 0;
        self.workfront_count = 0;
        self.floor_column_data.clear();
        self.needs_recalc = false;
        self.step_elements.clear();
    }

    /// Set step data from Python step table
    pub fn set_step_data(&mut self, max_step: usize) {
        self.max_step = max_step;
        self.current_step = 1;
        self.step_input = "1".to_string();
    }
}

impl Default for UiState {
    fn default() -> Self {
        Self::new()
    }
}

/// Render the header with buttons
pub fn render_header(ui: &mut egui::Ui, state: &mut UiState) {
    ui.horizontal(|ui| {
        // File open button
        if ui.button("📁 Open File").clicked() {
            state.status_message = "Opening file dialog...".to_string();
            // Note: Native file dialog would require additional setup
            // For now, we'll use a simple approach
        }

        // Recalc button - triggers construction calculation
        // Highlighted when needs_recalc is true (after load or reset)
        let recalc_enabled = state.has_data;
        let recalc_button = if state.needs_recalc && recalc_enabled {
            // Highlighted button - orange/yellow to draw attention
            egui::Button::new("🔄 Recalc").fill(egui::Color32::from_rgb(255, 180, 50))
        } else {
            egui::Button::new("🔄 Recalc")
        };

        if ui.add_enabled(recalc_enabled, recalc_button).clicked() {
            // Signal that recalc was requested - actual calculation happens in lib.rs
            state.status_message = "Calculating construction sequence...".to_string();
        }

        // Reset button - clears state
        if ui.button("🗑 Reset").clicked() {
            state.reset();
            state.status_message = "Reset complete".to_string();
        }

        ui.separator();

        // Mode toggle - Development/Simulation
        ui.label("Mode:");

        // Development radio button (always enabled)
        ui.radio_value(&mut state.mode, "Development".to_string(), "Development");

        // Simulation radio button (disabled - shown but not functional)
        // Note: Simulation mode is intentionally disabled per requirements
        ui.add_enabled(
            false,
            egui::Label::new("Simulation").sense(egui::Sense::click()),
        );

        // If somehow simulation was selected, revert to Development
        if state.mode == "Simulation" {
            state.mode = "Development".to_string();
        }

        ui.separator();

        // ID labels toggle
        ui.checkbox(&mut state.show_id_labels, "Show IDs");

        // Step navigation (only show if we have steps)
        if state.max_step > 0 {
            ui.separator();

            // Step label
            ui.label(format!("Step {}/{}", state.current_step, state.max_step));

            // Previous button
            let prev_enabled = state.current_step > 1;
            if ui
                .add_enabled(prev_enabled, egui::Button::new("◀"))
                .clicked()
            {
                state.current_step = state.current_step.saturating_sub(1);
                state.step_input = state.current_step.to_string();
            }

            // Slider for step navigation
            let mut step_slider = state.current_step;
            ui.add(
                egui::Slider::new(&mut step_slider, 1..=state.max_step)
                    .step_by(1.0)
                    .show_value(false),
            );
            if step_slider != state.current_step {
                state.current_step = step_slider;
                state.step_input = state.current_step.to_string();
            }

            // Next button
            let next_enabled = state.current_step < state.max_step;
            if ui
                .add_enabled(next_enabled, egui::Button::new("▶"))
                .clicked()
            {
                state.current_step = (state.current_step + 1).min(state.max_step);
                state.step_input = state.current_step.to_string();
            }

            // Direct input field
            let step_input_response = egui::TextEdit::singleline(&mut state.step_input)
                .desired_width(50.0)
                .ui(ui);

            // Parse input on Enter or when focus is lost
            if step_input_response.lost_focus() {
                if let Ok(step) = state.step_input.parse::<usize>() {
                    let clamped = step.max(1).min(state.max_step);
                    state.current_step = clamped;
                    state.step_input = clamped.to_string();
                } else {
                    // Invalid input - reset to current step
                    state.step_input = state.current_step.to_string();
                }
            }
        }
    });
}

/// Render the status panel on the left
pub fn render_status_panel(ui: &mut egui::Ui, state: &UiState) {
    ui.heading("Status");
    ui.separator();

    ui.vertical(|ui| {
        ui.label(format!("Mode: {}", state.mode));
        ui.label(format!("Status: {}", state.status_message));

        if state.has_data {
            ui.label("Data loaded: Yes");
            ui.label(format!(
                "Validation: {}",
                if state.validation_passed {
                    "Passed"
                } else {
                    "Not validated"
                }
            ));
        } else {
            ui.label("Data loaded: No");
        }

        if !state.file_path.is_empty() {
            ui.label(format!("File: {}", state.file_path));
        }
    });
}

/// Render tabs in the view panel
pub fn render_tabs(ui: &mut egui::Ui, state: &mut UiState) {
    ui.horizontal(|ui| {
        ui.selectable_value(&mut state.current_tab, "Settings".to_string(), "Settings");
        ui.selectable_value(&mut state.current_tab, "View".to_string(), "View");
        ui.selectable_value(&mut state.current_tab, "Result".to_string(), "Result");
    });
    ui.separator();
}

/// Render the settings tab content
pub fn render_settings_tab(ui: &mut egui::Ui, state: &mut UiState) {
    ui.heading("Settings");
    ui.separator();

    ui.vertical(|ui| {
        ui.checkbox(&mut state.show_id_labels, "Show ID Labels");

        ui.label("View Options:");
        ui.indent("view_opts", |ui| {
            ui.label("Grid: Enabled");
            ui.label("Nodes: Enabled");
            ui.label("Elements: Enabled");
        });
    });
}

/// Render the view tab content
pub fn render_view_tab(ui: &mut egui::Ui, _state: &UiState) {
    ui.heading("View");
    ui.separator();

    ui.label("3D Viewport");
    ui.label("(Render area for structure visualization)");
}

/// Render the result tab content
pub fn render_result_tab(ui: &mut egui::Ui, state: &UiState) {
    ui.heading("Result");
    ui.separator();

    if !state.has_data {
        ui.label("No data loaded. Please open a file first.");
        return;
    }

    // Show warning if recalculation needed
    if state.needs_recalc {
        ui.horizontal(|ui| {
            ui.colored_label(
                egui::Color32::from_rgb(255, 180, 50),
                "⚠ Construction sequence not calculated. Press Recalc button.",
            );
        });
        ui.add_space(10.0);
        ui.separator();
    }

    // Validation Status
    ui.horizontal(|ui| {
        ui.label("Validation Status:");
        if state.validation_passed {
            ui.colored_label(egui::Color32::from_rgb(100, 200, 100), "✓ PASSED");
        } else if state.needs_recalc {
            ui.colored_label(egui::Color32::from_rgb(200, 200, 100), "⏳ PENDING");
        } else {
            ui.colored_label(egui::Color32::from_rgb(200, 100, 100), "✗ FAILED");
        }
    });

    ui.add_space(10.0);
    ui.separator();

    // ========================================================================
    // Construction Summary
    // ========================================================================
    ui.heading("Construction Summary");
    ui.add_space(5.0);

    // Summary grid
    egui::Grid::new("summary_grid")
        .num_columns(2)
        .spacing([20.0, 4.0])
        .show(ui, |ui| {
            ui.label("Total Elements:");
            ui.label(format!("{}", state.total_elements));
            ui.end_row();

            ui.label("Columns:");
            ui.label(format!("{}", state.total_columns));
            ui.end_row();

            ui.label("Girders:");
            ui.label(format!("{}", state.total_girders));
            ui.end_row();

            ui.label("Workfronts:");
            ui.label(format!("{}", state.workfront_count));
            ui.end_row();

            ui.label("Total Steps:");
            ui.label(format!("{}", state.max_step));
            ui.end_row();
        });

    ui.add_space(10.0);

    // Overall progress bar
    let progress = if state.max_step > 0 {
        state.current_step as f32 / state.max_step as f32
    } else {
        0.0
    };
    ui.label(format!(
        "Current Step: {} / {}",
        state.current_step, state.max_step
    ));
    ui.add(
        egui::ProgressBar::new(progress)
            .text(format!("{:.0}%", progress * 100.0))
            .animate(false),
    );

    ui.add_space(15.0);
    ui.separator();

    // ========================================================================
    // Floor Column Installation Rates
    // ========================================================================
    ui.heading("Floor Column Installation");
    ui.add_space(5.0);

    if state.floor_column_data.is_empty() {
        ui.label("No floor data available.");
    } else {
        egui::ScrollArea::vertical()
            .max_height(200.0)
            .show(ui, |ui| {
                egui::Grid::new("floor_grid")
                    .num_columns(3)
                    .spacing([15.0, 4.0])
                    .striped(true)
                    .show(ui, |ui| {
                        // Header
                        ui.strong("Floor");
                        ui.strong("Columns");
                        ui.strong("Progress");
                        ui.end_row();

                        // Data rows
                        for (floor, total, installed) in &state.floor_column_data {
                            let rate = if *total > 0 {
                                *installed as f32 / *total as f32
                            } else {
                                0.0
                            };

                            ui.label(format!("F{}", floor));
                            ui.label(format!("{} / {}", installed, total));
                            ui.add(
                                egui::ProgressBar::new(rate)
                                    .text(format!("{:.0}%", rate * 100.0))
                                    .desired_width(100.0),
                            );
                            ui.end_row();
                        }
                    });
            });
    }

    ui.add_space(15.0);
    ui.separator();

    // ========================================================================
    // Element Type Distribution
    // ========================================================================
    ui.heading("Element Distribution");
    ui.add_space(5.0);

    if state.total_elements > 0 {
        let column_ratio = state.total_columns as f32 / state.total_elements as f32;
        let girder_ratio = state.total_girders as f32 / state.total_elements as f32;

        ui.horizontal(|ui| {
            ui.label("Columns:");
            ui.add(
                egui::ProgressBar::new(column_ratio)
                    .text(format!("{:.1}%", column_ratio * 100.0))
                    .fill(egui::Color32::from_rgb(100, 150, 200))
                    .desired_width(150.0),
            );
        });

        ui.horizontal(|ui| {
            ui.label("Girders:");
            ui.add(
                egui::ProgressBar::new(girder_ratio)
                    .text(format!("{:.1}%", girder_ratio * 100.0))
                    .fill(egui::Color32::from_rgb(200, 150, 100))
                    .desired_width(150.0),
            );
        });
    }
}

/// Main UI rendering function that creates the full layout
pub fn render(ui_state: &mut UiState, ctx: &egui::Context) {
    // Top panel - Header with buttons
    egui::TopBottomPanel::top("header").show(ctx, |ui| {
        egui::menu::bar(ui, |ui| {
            ui.label("AssyPlan - Development Mode");
            ui.separator();
            render_header(ui, ui_state);
        });
    });

    // Left panel - Status
    egui::SidePanel::left("status_panel")
        .default_width(200.0)
        .show(ctx, |ui| {
            render_status_panel(ui, ui_state);
        });

    // Central panel - View with tabs
    egui::CentralPanel::default().show(ctx, |ui| {
        render_tabs(ui, ui_state);

        match ui_state.current_tab.as_str() {
            "Settings" => render_settings_tab(ui, ui_state),
            "View" => render_view_tab(ui, ui_state),
            "Result" => render_result_tab(ui, ui_state),
            _ => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ui_state_new() {
        let state = UiState::new();
        assert!(state.show_id_labels);
        assert_eq!(state.mode, "Development");
        assert!(!state.has_data);
        assert!(!state.validation_passed);
        assert_eq!(state.current_step, 1);
        assert_eq!(state.max_step, 0);
        assert_eq!(state.step_input, "1");
        assert_eq!(state.display_mode, DisplayMode::Model);
        assert!(!state.has_step_data);
    }

    #[test]
    fn test_ui_state_reset() {
        let mut state = UiState::new();
        state.show_id_labels = false;
        state.mode = "Simulation".to_string();
        state.file_path = "test.csv".to_string();
        state.has_data = true;
        state.validation_passed = true;
        state.current_step = 5;
        state.max_step = 10;
        state.step_input = "5".to_string();
        state.display_mode = DisplayMode::Construction;
        state.has_step_data = true;

        state.reset();

        assert!(state.show_id_labels);
        assert_eq!(state.mode, "Development");
        assert!(state.file_path.is_empty());
        assert!(!state.has_data);
        assert!(!state.validation_passed);
        assert_eq!(state.current_step, 1);
        assert_eq!(state.max_step, 0);
        assert_eq!(state.step_input, "1");
        assert_eq!(state.display_mode, DisplayMode::Model);
        assert!(!state.has_step_data);
    }

    #[test]
    fn test_set_step_data() {
        let mut state = UiState::new();

        // Initial state
        assert_eq!(state.current_step, 1);
        assert_eq!(state.max_step, 0);

        // Set step data
        state.set_step_data(5);

        assert_eq!(state.current_step, 1);
        assert_eq!(state.max_step, 5);
        assert_eq!(state.step_input, "1");
    }

    #[test]
    fn test_set_step_data_overwrites() {
        let mut state = UiState::new();

        state.set_step_data(5);
        state.current_step = 3;
        state.step_input = "3".to_string();

        // Setting new step data should reset to step 1
        state.set_step_data(10);

        assert_eq!(state.current_step, 1);
        assert_eq!(state.max_step, 10);
        assert_eq!(state.step_input, "1");
    }
}
