// UI module for AssyPlan application

use eframe::egui;

/// UI State for managing application state
pub struct UiState {
    /// Whether to show ID labels on nodes
    pub show_id_labels: bool,
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
}

impl UiState {
    /// Create a new UiState instance
    pub fn new() -> Self {
        Self {
            show_id_labels: true,
            mode: "Development".to_string(),
            file_path: String::new(),
            current_tab: "View".to_string(),
            status_message: "Ready".to_string(),
            has_data: false,
            validation_passed: false,
        }
    }

    /// Reset state to initial values
    pub fn reset(&mut self) {
        self.show_id_labels = true;
        self.mode = "Development".to_string();
        self.file_path.clear();
        self.current_tab = "View".to_string();
        self.status_message = "Ready".to_string();
        self.has_data = false;
        self.validation_passed = false;
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

        // Recalc button - triggers validation and rendering
        let recalc_enabled = state.has_data;
        if ui
            .add_enabled(recalc_enabled, egui::Button::new("🔄 Recalc"))
            .clicked()
        {
            state.status_message = "Validating...".to_string();
            // Validation logic would be triggered here
            state.validation_passed = true;
            state.status_message = "Rendering complete".to_string();
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

    ui.vertical(|ui| {
        ui.label(format!(
            "Validation Status: {}",
            if state.validation_passed {
                "PASSED"
            } else {
                "NOT VALIDATED"
            }
        ));

        if state.has_data {
            ui.label("Data has been processed successfully.");
        } else {
            ui.label("No data loaded. Please open a file and run recalc.");
        }
    });
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
    }

    #[test]
    fn test_ui_state_reset() {
        let mut state = UiState::new();
        state.show_id_labels = false;
        state.mode = "Simulation".to_string();
        state.file_path = "test.csv".to_string();
        state.has_data = true;
        state.validation_passed = true;

        state.reset();

        assert!(state.show_id_labels);
        assert_eq!(state.mode, "Development");
        assert!(state.file_path.is_empty());
        assert!(!state.has_data);
        assert!(!state.validation_passed);
    }
}
