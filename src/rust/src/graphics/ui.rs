// UI module for AssyPlan application

use eframe::egui::{self};

// ============================================================================
// Simulation Mode Data Types (Phase 3)
// ============================================================================

/// Grid configuration for simulation mode
#[derive(Clone, Debug)]
pub struct GridConfig {
    /// Number of grid lines in x direction (columns)
    pub nx: usize,
    /// Number of grid lines in y direction (rows)
    pub ny: usize,
    /// Number of floor levels (z levels, z=0 is ground)
    pub nz: usize,
    /// Floor height interval (constant, in same units as coordinates)
    pub dz: f64,
    /// X grid spacing
    pub dx: f64,
    /// Y grid spacing
    pub dy: f64,
}

impl Default for GridConfig {
    fn default() -> Self {
        Self {
            nx: 4,
            ny: 8,
            nz: 3,
            dz: 4000.0,
            dx: 6000.0,
            dy: 6000.0,
        }
    }
}

/// A workfront starting position (grid intersection, 0-indexed)
#[derive(Clone, Debug, PartialEq)]
pub struct SimWorkfront {
    /// 1-indexed workfront ID
    pub id: i32,
    /// Grid x index (0-indexed, 0..nx-1)
    pub grid_x: usize,
    /// Grid y index (0-indexed, 0..ny-1)
    pub grid_y: usize,
}

/// Early termination reason for a simulation scenario
#[derive(Clone, Debug, PartialEq)]
pub enum TerminationReason {
    Completed,
    Cancelled,
    UpperFloorViolation,
    NoProgress,
    IndependentOveruse,
    NoCandidates,
    MaxIterations,
}

impl std::fmt::Display for TerminationReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TerminationReason::Completed => write!(f, "Completed"),
            TerminationReason::Cancelled => write!(f, "Cancelled"),
            TerminationReason::UpperFloorViolation => write!(f, "Upper Floor Violation"),
            TerminationReason::NoProgress => write!(f, "No Progress"),
            TerminationReason::IndependentOveruse => write!(f, "Independent Overuse"),
            TerminationReason::NoCandidates => write!(f, "No Candidates"),
            TerminationReason::MaxIterations => write!(f, "Max Iterations"),
        }
    }
}

/// Metrics for a single simulation scenario
#[derive(Clone, Debug)]
pub struct ScenarioMetrics {
    pub avg_members_per_step: f64,
    pub avg_connectivity: f64,
    pub total_steps: usize,
    pub total_members_installed: usize,
    pub termination_reason: TerminationReason,
    pub throttle_events: usize,
    pub floor_rebase_events: usize,
    pub spatial_rebase_events: usize,
}

/// A single element installation entry in the global sequence order
#[derive(Clone, Debug)]
pub struct SimSequence {
    /// Element ID installed at this sequence position (1-indexed element ID)
    pub element_id: i32,
    /// Global sequence number across the whole scenario (1-indexed)
    pub sequence_number: usize,
}

/// A single workfront's completed pattern within a global step.
#[derive(Clone, Debug)]
pub struct LocalStep {
    /// Workfront ID that completed this local pattern
    pub workfront_id: i32,
    /// Element IDs installed by this workfront (1-indexed)
    pub element_ids: Vec<i32>,
    /// Floor level of this local step
    pub floor: i32,
    /// Pattern name (e.g. "ColGirder", "Bootstrap")
    pub pattern: String,
}

/// A single step in a simulation scenario.
/// May contain multiple local steps from different workfronts that were
/// completed in the same global step cycle.
#[derive(Clone, Debug)]
pub struct SimStep {
    /// Representative workfront ID (first local step's workfront)
    pub workfront_id: i32,
    /// All element IDs installed in this step (union of all local steps)
    pub element_ids: Vec<i32>,
    /// Collated sequence entries (round-robin across local steps)
    pub sequences: Vec<SimSequence>,
    /// Minimum floor level across all local steps
    pub floor: i32,
    /// Summary pattern name
    pub pattern: String,
    /// Individual local steps that compose this global step
    pub local_steps: Vec<LocalStep>,
}

impl SimStep {
    /// Build a step and auto-generate global sequence entries.
    pub fn from_elements(
        workfront_id: i32,
        element_ids: Vec<i32>,
        floor: i32,
        pattern: impl Into<String>,
        start_seq: usize,
    ) -> Self {
        let pat = pattern.into();
        let sequences = element_ids
            .iter()
            .enumerate()
            .map(|(offset, &element_id)| SimSequence {
                element_id,
                sequence_number: start_seq + offset,
            })
            .collect();

        let local_steps = vec![LocalStep {
            workfront_id,
            element_ids: element_ids.clone(),
            floor,
            pattern: pat.clone(),
        }];

        Self {
            workfront_id,
            element_ids,
            sequences,
            floor,
            pattern: pat,
            local_steps,
        }
    }

    /// Build a step from already-assigned sequence entries.
    pub fn from_sequences(
        workfront_id: i32,
        sequences: Vec<SimSequence>,
        floor: i32,
        pattern: impl Into<String>,
    ) -> Self {
        let element_ids = sequences.iter().map(|seq| seq.element_id).collect::<Vec<_>>();
        let pat = pattern.into();

        let local_steps = vec![LocalStep {
            workfront_id,
            element_ids: element_ids.clone(),
            floor,
            pattern: pat.clone(),
        }];

        Self {
            workfront_id,
            element_ids,
            sequences,
            floor,
            pattern: pat,
            local_steps,
        }
    }

    /// Build a global step by merging multiple local steps.
    /// Sequences are collated round-robin: round 0 picks element[0] from each
    /// local step, round 1 picks element[1], etc. All elements in the same
    /// round share the same sequence_number. Shorter local steps simply have
    /// no entry in later rounds.
    pub fn from_local_steps(local_steps: Vec<LocalStep>, start_seq: usize) -> Self {
        assert!(!local_steps.is_empty(), "from_local_steps requires at least one local step");

        let workfront_id = local_steps[0].workfront_id;
        let floor = local_steps.iter().map(|ls| ls.floor).min().unwrap_or(1);
        let pattern = if local_steps.len() == 1 {
            local_steps[0].pattern.clone()
        } else {
            format!("Multi({})", local_steps.len())
        };

        let max_len = local_steps.iter().map(|ls| ls.element_ids.len()).max().unwrap_or(0);
        let mut sequences = Vec::new();
        let mut element_ids = Vec::new();

        for round in 0..max_len {
            let seq_num = start_seq + round;
            for ls in &local_steps {
                if let Some(&eid) = ls.element_ids.get(round) {
                    sequences.push(SimSequence {
                        element_id: eid,
                        sequence_number: seq_num,
                    });
                    element_ids.push(eid);
                }
            }
        }

        Self {
            workfront_id,
            element_ids,
            sequences,
            floor,
            pattern,
            local_steps,
        }
    }

    /// Number of sequence rounds in this step (max local step member count).
    pub fn sequence_round_count(&self) -> usize {
        self.local_steps.iter().map(|ls| ls.element_ids.len()).max().unwrap_or(0)
    }
}

/// A complete simulation scenario (one sequence of steps)
#[derive(Clone, Debug)]
pub struct SimScenario {
    /// Scenario ID (1-indexed)
    pub id: usize,
    /// Random seed used
    pub seed: u64,
    /// All steps in this scenario
    pub steps: Vec<SimStep>,
    /// Scenario metrics
    pub metrics: ScenarioMetrics,
}

/// View mode for the graphics display
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DisplayMode {
    /// Show all elements (full model)
    Model,
    /// Show elements step-by-step (construction sequence)
    Construction,
    /// Simulation mode - auto-generate construction sequences
    Simulation,
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
    /// Visibility: show hidden (inactive/uninstalled) nodes & elements in ghost style.
    /// Only applies in Construction display mode.
    pub show_hidden: bool,
    /// Display mode: Model (full) or Construction (step-by-step)
    pub display_mode: DisplayMode,
    /// Whether step data has been calculated
    pub has_step_data: bool,
    /// Whether sequence data has been calculated and can be visualized
    pub has_sequence_data: bool,
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
    /// Upper-floor column installation threshold (0.0~1.0)
    /// Floor N columns cannot start until floor N-1 reaches this rate.
    /// Used for metric visualization only (simulation constraint applied in Phase 2).
    pub upper_floor_threshold: f64,
    /// Lower-floor column completion ratio threshold (0.0~1.0)
    /// When lower floor column completion reaches this ratio,
    /// upper-floor candidates receive additional score bonus.
    pub lower_floor_completion_ratio: f64,
    /// Lower-floor forced completion threshold (member count)
    /// If remaining members on a lower floor are <= this value,
    /// lower-floor completion is strongly preferred.
    pub lower_floor_forced_completion: usize,

    // ============================================================================
    // Simulation Mode State (Phase 3)
    // ============================================================================
    /// Grid configuration for simulation mode
    pub grid_config: GridConfig,
    /// Selected workfront positions (grid indices, 0-based)
    pub sim_workfronts: Vec<SimWorkfront>,
    /// Generated simulation scenarios
    pub sim_scenarios: Vec<SimScenario>,
    /// Currently selected scenario index (0-based, None if none selected)
    pub sim_selected_scenario: Option<usize>,
    /// Whether simulation is currently playing back
    pub sim_playing: bool,
    /// Playback speed multiplier (1, 2, 4)
    pub sim_speed: u32,
    /// Current playback step in the selected scenario (1-indexed)
    pub sim_current_step: usize,
    /// Timer accumulator for auto-playback (seconds)
    pub sim_play_timer: f64,
    /// Algorithm weights (w1=부재수, w2=연결성, w3=거리)
    pub sim_weights: (f64, f64, f64),
    /// Whether simulation is currently running (calculating)
    pub sim_running: bool,
    /// Number of scenarios to generate
    pub sim_scenario_count: usize,
    /// Flag: export debug files requested (set by UI button, consumed by app update loop)
    pub sim_export_requested: bool,
    /// Optional selected scenario index for export. None means export all scenarios.
    pub sim_export_selected_index: Option<usize>,
    /// Last export status message (shown in UI after export)
    pub sim_export_status: String,
    /// Navigation mode in sim 3D view: true = Sequence mode, false = Step mode
    pub sim_nav_sequence_mode: bool,
    /// Current sequence position in Sequence nav mode (1-indexed, global across all steps)
    pub sim_current_sequence: usize,
    /// Sim View sub-mode: true = Model (all elements active, no ghost), false = Construction
    pub sim_view_is_model: bool,
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
            show_hidden: true,
            display_mode: DisplayMode::Model,
            has_step_data: false,
            has_sequence_data: false,
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
            upper_floor_threshold: 0.3,
            lower_floor_completion_ratio: 0.8,
            lower_floor_forced_completion: 10,
            // Simulation Mode (Phase 3)
            grid_config: GridConfig::default(),
            sim_workfronts: Vec::new(),
            sim_scenarios: Vec::new(),
            sim_selected_scenario: None,
            sim_playing: false,
            sim_speed: 1,
            sim_current_step: 1,
            sim_play_timer: 0.0,
            sim_weights: (0.5, 0.3, 0.2),
            sim_running: false,
            sim_scenario_count: 2,
            sim_export_requested: false,
            sim_export_selected_index: None,
            sim_export_status: String::new(),
            sim_nav_sequence_mode: false,
            sim_current_sequence: 1,
            sim_view_is_model: false,
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
        self.show_hidden = true;
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
        // Reset simulation state
        self.sim_workfronts.clear();
        self.sim_scenarios.clear();
        self.sim_selected_scenario = None;
        self.sim_playing = false;
        self.sim_speed = 1;
        self.sim_current_step = 1;
        self.sim_play_timer = 0.0;
        self.sim_running = false;
        self.sim_export_requested = false;
        self.sim_export_selected_index = None;
        self.sim_export_status.clear();
        self.sim_nav_sequence_mode = false;
        self.sim_current_sequence = 1;
        self.sim_view_is_model = false;
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

/// Render the result tab content
pub fn render_result_tab(ui: &mut egui::Ui, state: &UiState) {
    ui.heading("Result");
    ui.separator();

    if !state.has_data {
        ui.label("No data loaded. Please open a file first.");
        return;
    }

    egui::ScrollArea::vertical()
        .id_source("result_tab_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            render_result_tab_inner(ui, state);
        }); // end ScrollArea
}

/// Inner content of the result tab (wrapped in ScrollArea by render_result_tab)
fn render_result_tab_inner(ui: &mut egui::Ui, state: &UiState) {
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
    // Element Type Distribution  (섹션 1 - 먼저 출력)
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
    } else {
        ui.label("No element data available.");
    }

    ui.add_space(15.0);
    ui.separator();

    // ========================================================================
    // Construction Summary  (섹션 2)
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
    // Floor Column Installation Rates  (섹션 3)
    // ========================================================================
    ui.heading("Floor Column Installation");
    ui.add_space(5.0);

    if state.floor_column_data.is_empty() {
        ui.label("No floor data available.");
    } else {
        egui::ScrollArea::vertical()
            .id_source("floor_scroll")
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
    // Charts  (섹션 4)
    // ========================================================================

    // Only show charts when step data is available
    if state.max_step == 0 || state.step_elements.is_empty() {
        ui.label("No step data. Press Recalc to generate charts.");
        return;
    }

    // ---- Chart 1: 누적 총 부재 설치 갯수 per step ----
    ui.heading("Cumulative Elements Installed");
    ui.add_space(5.0);

    // Build cumulative data: (step as f32, cumulative_count as f32)
    let mut cum: usize = 0;
    let cumulative_data: Vec<(f32, f32)> = (1..=state.max_step)
        .map(|s| {
            if s < state.step_elements.len() {
                cum += state.step_elements[s].len();
            }
            (s as f32, cum as f32)
        })
        .collect();

    let max_cum = cumulative_data.last().map(|p| p.1).unwrap_or(1.0).max(1.0);
    let chart1_height = 160.0f32;
    let chart_padding_left = 50.0f32;
    let chart_padding_bottom = 24.0f32;
    let chart_padding_top = 10.0f32;
    let chart_padding_right = 20.0f32;
    let chart_margin_h = 12.0f32; // horizontal outer margin

    egui::Frame::none()
        .inner_margin(egui::Margin::symmetric(chart_margin_h, 0.0))
        .show(ui, |ui| {
            let (chart1_rect, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), chart1_height),
                egui::Sense::hover(),
            );

            {
                let painter = ui.painter_at(chart1_rect);

                // Background
                painter.rect_filled(chart1_rect, 2.0, egui::Color32::from_rgb(30, 30, 40));

                let plot_rect = egui::Rect::from_min_max(
                    egui::pos2(
                        chart1_rect.left() + chart_padding_left,
                        chart1_rect.top() + chart_padding_top,
                    ),
                    egui::pos2(
                        chart1_rect.right() - chart_padding_right,
                        chart1_rect.bottom() - chart_padding_bottom,
                    ),
                );

                // Axes
                let axis_color = egui::Color32::from_rgb(160, 160, 160);
                let grid_color = egui::Color32::from_rgb(60, 60, 70);

                // Y-axis
                painter.line_segment(
                    [plot_rect.left_bottom(), plot_rect.left_top()],
                    egui::Stroke::new(1.0, axis_color),
                );
                // X-axis
                painter.line_segment(
                    [plot_rect.left_bottom(), plot_rect.right_bottom()],
                    egui::Stroke::new(1.0, axis_color),
                );

                // Y-axis label: 0 and max
                let font_id = egui::FontId::proportional(10.0);
                painter.text(
                    egui::pos2(chart1_rect.left(), plot_rect.bottom() - 5.0),
                    egui::Align2::LEFT_CENTER,
                    "0",
                    font_id.clone(),
                    axis_color,
                );
                painter.text(
                    egui::pos2(chart1_rect.left(), plot_rect.top()),
                    egui::Align2::LEFT_CENTER,
                    format!("{}", max_cum as usize),
                    font_id.clone(),
                    axis_color,
                );

                // Horizontal grid line at 50%
                let mid_y = plot_rect.bottom() - plot_rect.height() * 0.5;
                painter.line_segment(
                    [
                        egui::pos2(plot_rect.left(), mid_y),
                        egui::pos2(plot_rect.right(), mid_y),
                    ],
                    egui::Stroke::new(0.5, grid_color),
                );

                // Helper: data coords -> screen coords
                let max_step_f = state.max_step as f32;
                let to_screen = |step: f32, val: f32| -> egui::Pos2 {
                    let x = plot_rect.left()
                        + (step - 1.0) / (max_step_f - 1.0).max(1.0) * plot_rect.width();
                    let y = plot_rect.bottom() - (val / max_cum) * plot_rect.height();
                    egui::pos2(x, y)
                };

                // Data line
                if cumulative_data.len() >= 2 {
                    let points: Vec<egui::Pos2> = cumulative_data
                        .iter()
                        .map(|(s, v)| to_screen(*s, *v))
                        .collect();
                    painter.add(egui::Shape::line(
                        points,
                        egui::Stroke::new(2.0, egui::Color32::from_rgb(100, 200, 255)),
                    ));
                }

                // Data points
                for (s, v) in &cumulative_data {
                    painter.circle_filled(
                        to_screen(*s, *v),
                        2.5,
                        egui::Color32::from_rgb(100, 200, 255),
                    );
                }

                // X-axis labels: show up to 10 evenly spaced step labels
                let label_step = (state.max_step / 10).max(1);
                for s in (1..=state.max_step).step_by(label_step) {
                    let x = plot_rect.left()
                        + (s as f32 - 1.0) / (max_step_f - 1.0).max(1.0) * plot_rect.width();
                    painter.text(
                        egui::pos2(x, chart1_rect.bottom() - 4.0),
                        egui::Align2::CENTER_BOTTOM,
                        format!("{}", s),
                        font_id.clone(),
                        axis_color,
                    );
                }
            }

            ui.add_space(10.0);
        }); // end Chart 1 Frame
    ui.separator();

    // ---- Chart 2: 층별 기둥 설치율 per step ----
    ui.heading("Floor Column Installation Rate by Step");
    ui.add_space(5.0);

    // Collect unique floors from floor_column_data (sorted ascending)
    let mut floors: Vec<i32> = state.floor_column_data.iter().map(|(f, _, _)| *f).collect();
    floors.sort();
    floors.dedup();

    // Build floor totals map: floor -> total_columns
    let floor_totals: std::collections::HashMap<i32, usize> = state
        .floor_column_data
        .iter()
        .map(|(f, total, _)| (*f, *total))
        .collect();

    // Compute per-step cumulative column count per floor
    // floor_step_rate[floor_idx][step_idx (0-based)] = rate (0.0..=1.0)
    let floor_step_rates: Vec<Vec<f32>> = floors
        .iter()
        .map(|floor| {
            let total = *floor_totals.get(floor).unwrap_or(&1);
            let total_f = total.max(1) as f32;
            let mut cum_count: usize = 0;
            (1..=state.max_step)
                .map(|s| {
                    if s < state.step_elements.len() {
                        for (_eid, mtype, efloor) in &state.step_elements[s] {
                            if mtype == "Column" && *efloor == *floor {
                                cum_count += 1;
                            }
                        }
                    }
                    (cum_count as f32 / total_f).min(1.0)
                })
                .collect()
        })
        .collect();

    // Floor line colors (shared by Chart 2 and Chart 3)
    let floor_colors = [
        egui::Color32::from_rgb(255, 100, 100),
        egui::Color32::from_rgb(255, 180, 60),
        egui::Color32::from_rgb(100, 220, 100),
        egui::Color32::from_rgb(80, 180, 255),
        egui::Color32::from_rgb(200, 100, 255),
        egui::Color32::from_rgb(255, 120, 200),
        egui::Color32::from_rgb(100, 240, 220),
        egui::Color32::from_rgb(255, 240, 100),
    ];

    let chart2_height = 180.0f32;
    egui::Frame::none()
        .inner_margin(egui::Margin::symmetric(chart_margin_h, 0.0))
        .show(ui, |ui| {
            let (chart2_rect, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), chart2_height),
                egui::Sense::hover(),
            );

            // Floor line colors (cycle through a palette)
            {
                let painter = ui.painter_at(chart2_rect);

                // Background
                painter.rect_filled(chart2_rect, 2.0, egui::Color32::from_rgb(30, 30, 40));

                // Reserve right side for legend
                let legend_width = 70.0f32;
                let plot_rect = egui::Rect::from_min_max(
                    egui::pos2(
                        chart2_rect.left() + chart_padding_left,
                        chart2_rect.top() + chart_padding_top,
                    ),
                    egui::pos2(
                        chart2_rect.right() - chart_padding_right - legend_width,
                        chart2_rect.bottom() - chart_padding_bottom,
                    ),
                );

                let axis_color = egui::Color32::from_rgb(160, 160, 160);
                let grid_color = egui::Color32::from_rgb(60, 60, 70);
                let font_id = egui::FontId::proportional(10.0);

                // Y-axis
                painter.line_segment(
                    [plot_rect.left_bottom(), plot_rect.left_top()],
                    egui::Stroke::new(1.0, axis_color),
                );
                // X-axis
                painter.line_segment(
                    [plot_rect.left_bottom(), plot_rect.right_bottom()],
                    egui::Stroke::new(1.0, axis_color),
                );

                // Y-axis labels: 0%, 50%, 100%
                for (frac, label) in &[(0.0f32, "0%"), (0.5, "50%"), (1.0, "100%")] {
                    let y = plot_rect.bottom() - frac * plot_rect.height();
                    painter.line_segment(
                        [
                            egui::pos2(plot_rect.left(), y),
                            egui::pos2(plot_rect.right(), y),
                        ],
                        egui::Stroke::new(if *frac == 1.0 { 0.8 } else { 0.5 }, grid_color),
                    );
                    painter.text(
                        egui::pos2(chart2_rect.left(), y),
                        egui::Align2::LEFT_CENTER,
                        *label,
                        font_id.clone(),
                        axis_color,
                    );
                }

                let max_step_f = state.max_step as f32;

                let to_screen2 = |step: f32, rate: f32| -> egui::Pos2 {
                    let x = plot_rect.left()
                        + (step - 1.0) / (max_step_f - 1.0).max(1.0) * plot_rect.width();
                    let y = plot_rect.bottom() - rate * plot_rect.height();
                    egui::pos2(x, y)
                };

                // Draw floor lines
                for (fi, (floor, rates)) in floors.iter().zip(floor_step_rates.iter()).enumerate() {
                    let color = floor_colors[fi % floor_colors.len()];

                    if rates.len() >= 2 {
                        let points: Vec<egui::Pos2> = rates
                            .iter()
                            .enumerate()
                            .map(|(i, rate)| to_screen2(i as f32 + 1.0, *rate))
                            .collect();
                        painter.add(egui::Shape::line(points, egui::Stroke::new(1.5, color)));
                    }

                    // Legend
                    let legend_x = chart2_rect.right() - legend_width + 5.0;
                    let legend_y = chart2_rect.top() + chart_padding_top + fi as f32 * 14.0;
                    painter.line_segment(
                        [
                            egui::pos2(legend_x, legend_y + 5.0),
                            egui::pos2(legend_x + 14.0, legend_y + 5.0),
                        ],
                        egui::Stroke::new(2.0, color),
                    );
                    painter.text(
                        egui::pos2(legend_x + 17.0, legend_y + 5.0),
                        egui::Align2::LEFT_CENTER,
                        format!("F{}", floor),
                        font_id.clone(),
                        color,
                    );
                }

                // X-axis labels
                let label_step = (state.max_step / 10).max(1);
                for s in (1..=state.max_step).step_by(label_step) {
                    let x = plot_rect.left()
                        + (s as f32 - 1.0) / (max_step_f - 1.0).max(1.0) * plot_rect.width();
                    painter.text(
                        egui::pos2(x, chart2_rect.bottom() - 4.0),
                        egui::Align2::CENTER_BOTTOM,
                        format!("{}", s),
                        font_id.clone(),
                        axis_color,
                    );
                }
            }

            ui.add_space(10.0);
        }); // end Chart 2 Frame

    ui.add_space(15.0);
    ui.separator();

    // ---- Chart 3: 상층부 기둥 설치율 (Upper-Floor Column Installation Rate) ----
    ui.heading("Upper-Floor Column Installation Rate");
    ui.add_space(3.0);
    ui.label(
        egui::RichText::new(format!(
            "Threshold: {:.0}%  —  each line F{{N+1}}/F{{N}} = upper(N+1) installed / lower(N) installed.",
            state.upper_floor_threshold * 100.0
        ))
        .weak()
        .small(),
    );
    ui.add_space(5.0);

    // floors sorted ascending (same as Chart 2)
    // For each floor N (except the top floor), compute per-step ratio:
    //   cum_installed(N+1) / cum_installed(N)
    // If cum(N) == 0, ratio = 0.0 (no columns on floor N yet, so upper rate is 0)
    let lower_floors: Vec<i32> = floors
        .iter()
        .copied()
        .filter(|&f| floors.contains(&(f + 1)))
        .collect();

    if lower_floors.is_empty() {
        ui.label("Only one floor detected — no upper-floor constraint applicable.");
    } else {
        let threshold_f = state.upper_floor_threshold as f32;

        // For each floor N, compute per-step: cum(N+1) / cum(N)
        // If cum(N) == 0, ratio = 0.0
        let upper_floor_rates: Vec<Vec<f32>> = lower_floors
            .iter()
            .map(|&floor_n| {
                let upper_floor = floor_n + 1;
                let mut cum_lower: usize = 0;
                let mut cum_upper: usize = 0;
                (1..=state.max_step)
                    .map(|s| {
                        if s < state.step_elements.len() {
                            for (_eid, mtype, efloor) in &state.step_elements[s] {
                                if mtype == "Column" {
                                    if *efloor == floor_n {
                                        cum_lower += 1;
                                    } else if *efloor == upper_floor {
                                        cum_upper += 1;
                                    }
                                }
                            }
                        }
                        if cum_lower == 0 {
                            0.0f32
                        } else {
                            cum_upper as f32 / cum_lower as f32
                        }
                    })
                    .collect()
            })
            .collect();

        let chart3_height = 180.0f32;
        egui::Frame::none()
            .inner_margin(egui::Margin::symmetric(chart_margin_h, 0.0))
            .show(ui, |ui| {
                let (chart3_rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), chart3_height),
                    egui::Sense::hover(),
                );

                {
                    let painter = ui.painter_at(chart3_rect);

                    // Background
                    painter.rect_filled(chart3_rect, 2.0, egui::Color32::from_rgb(30, 30, 40));

                    let legend_width = 70.0f32;
                    let plot_rect = egui::Rect::from_min_max(
                        egui::pos2(
                            chart3_rect.left() + chart_padding_left,
                            chart3_rect.top() + chart_padding_top,
                        ),
                        egui::pos2(
                            chart3_rect.right() - chart_padding_right - legend_width,
                            chart3_rect.bottom() - chart_padding_bottom,
                        ),
                    );

                    let axis_color = egui::Color32::from_rgb(160, 160, 160);
                    let grid_color = egui::Color32::from_rgb(60, 60, 70);
                    let font_id = egui::FontId::proportional(10.0);

                    // Dynamic Y-axis max: based on actual data range
                    let y_max: f32 = upper_floor_rates
                        .iter()
                        .flat_map(|r| r.iter().copied())
                        .fold(1.0f32, f32::max)
                        .max(1.0);

                    // Y-axis
                    painter.line_segment(
                        [plot_rect.left_bottom(), plot_rect.left_top()],
                        egui::Stroke::new(1.0, axis_color),
                    );
                    // X-axis
                    painter.line_segment(
                        [plot_rect.left_bottom(), plot_rect.right_bottom()],
                        egui::Stroke::new(1.0, axis_color),
                    );

                    // Y-axis grid + labels: 0, 0.5*y_max, y_max
                    for (frac, label) in &[
                        (0.0f32, format!("0")),
                        (0.5, format!("{:.1}", y_max * 0.5)),
                        (1.0, format!("{:.1}", y_max)),
                    ] {
                        let y = plot_rect.bottom() - frac * plot_rect.height();
                        painter.line_segment(
                            [
                                egui::pos2(plot_rect.left(), y),
                                egui::pos2(plot_rect.right(), y),
                            ],
                            egui::Stroke::new(if *frac == 1.0 { 0.8 } else { 0.5 }, grid_color),
                        );
                        painter.text(
                            egui::pos2(chart3_rect.left(), y),
                            egui::Align2::LEFT_CENTER,
                            label.as_str(),
                            font_id.clone(),
                            axis_color,
                        );
                    }

                    let max_step_f = state.max_step as f32;

                    let to_screen3 = |step: f32, ratio: f32| -> egui::Pos2 {
                        let x = plot_rect.left()
                            + (step - 1.0) / (max_step_f - 1.0).max(1.0) * plot_rect.width();
                        let y = plot_rect.bottom() - (ratio / y_max) * plot_rect.height();
                        egui::pos2(x, y)
                    };

                    // Threshold horizontal dashed line (threshold_f is 0~1, map to y_max scale)
                    let threshold_ratio = threshold_f * y_max;
                    let threshold_y =
                        plot_rect.bottom() - (threshold_ratio / y_max) * plot_rect.height();
                    // Draw dashed line manually (every 6px dash / 4px gap)
                    {
                        let dash_len = 6.0f32;
                        let gap_len = 4.0f32;
                        let mut x = plot_rect.left();
                        let threshold_color = egui::Color32::from_rgb(255, 80, 80);
                        while x < plot_rect.right() {
                            let x_end = (x + dash_len).min(plot_rect.right());
                            painter.line_segment(
                                [egui::pos2(x, threshold_y), egui::pos2(x_end, threshold_y)],
                                egui::Stroke::new(1.5, threshold_color),
                            );
                            x += dash_len + gap_len;
                        }
                        // Threshold label on right
                        painter.text(
                            egui::pos2(plot_rect.right() + 3.0, threshold_y),
                            egui::Align2::LEFT_CENTER,
                            format!("{:.0}%", threshold_f * 100.0),
                            font_id.clone(),
                            threshold_color,
                        );
                    }

                    // Draw floor lines: F{N} line = cum(N+1) / cum(N) ratio
                    for (fi, (&floor_n, rates)) in lower_floors
                        .iter()
                        .zip(upper_floor_rates.iter())
                        .enumerate()
                    {
                        let color = floor_colors[fi % floor_colors.len()];

                        if rates.len() >= 2 {
                            let points: Vec<egui::Pos2> = rates
                                .iter()
                                .enumerate()
                                .map(|(i, rate)| to_screen3(i as f32 + 1.0, *rate))
                                .collect();
                            painter.add(egui::Shape::line(points, egui::Stroke::new(1.5, color)));
                        }

                        // Legend: "F{N+1}/F{N}" = upper(N+1) installed / lower(N) installed
                        let legend_x = chart3_rect.right() - legend_width + 5.0;
                        let legend_y = chart3_rect.top() + chart_padding_top + fi as f32 * 14.0;
                        painter.line_segment(
                            [
                                egui::pos2(legend_x, legend_y + 5.0),
                                egui::pos2(legend_x + 14.0, legend_y + 5.0),
                            ],
                            egui::Stroke::new(2.0, color),
                        );
                        painter.text(
                            egui::pos2(legend_x + 17.0, legend_y + 5.0),
                            egui::Align2::LEFT_CENTER,
                            format!("F{}/F{}", floor_n + 1, floor_n),
                            font_id.clone(),
                            color,
                        );
                    }

                    // X-axis labels
                    let label_step = (state.max_step / 10).max(1);
                    for s in (1..=state.max_step).step_by(label_step) {
                        let x = plot_rect.left()
                            + (s as f32 - 1.0) / (max_step_f - 1.0).max(1.0) * plot_rect.width();
                        painter.text(
                            egui::pos2(x, chart3_rect.bottom() - 4.0),
                            egui::Align2::CENTER_BOTTOM,
                            format!("{}", s),
                            font_id.clone(),
                            axis_color,
                        );
                    }
                }

                ui.add_space(10.0);
            }); // end Chart 3 Frame
    }
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
