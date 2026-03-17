//! Simulation UI module — Grid settings panel and Workfront selector
//!
//! Entry points used from lib.rs:
//!   1. `render_sim_settings(ui, state)` — called from Settings tab when mode == Simulation
//!   2. `render_sim_view(ui, state)` — called from View tab (2-D grid plan, workfront clicks)
//!   3. `render_sim_result(ui, state)` — called from Result tab when mode == Simulation
//!
//! Note: 3-D member rendering is inlined directly in lib.rs View tab code.

use eframe::egui::{self, Color32, FontId, Pos2, Rect, Stroke, Ui, Vec2};

use crate::graphics::ui::{SimWorkfront, UiState};

// Shared chart layout constants (aligned with Development mode visuals)
const CHART_PADDING_LEFT: f32 = 50.0;
const CHART_PADDING_RIGHT: f32 = 20.0;
const CHART_PADDING_TOP: f32 = 10.0;
const CHART_PADDING_BOTTOM: f32 = 24.0;
const CHART_MARGIN_H: f32 = 12.0;
const CHART_LEGEND_W: f32 = 70.0;

// ============================================================================
// Settings tab (Grid config + algorithm weights + workfront list)
// ============================================================================

/// Render the Simulation settings panel.
/// Called from the "Settings" tab in lib.rs when mode == Simulation.
/// Returns true if the grid config changed (needs new pool rebuild).
pub fn render_sim_settings(ui: &mut Ui, state: &mut UiState) -> bool {
    let mut changed = false;

    ui.heading("Simulation — Grid Configuration");
    ui.separator();

    egui::Grid::new("sim_grid_config")
        .num_columns(2)
        .spacing([20.0, 6.0])
        .show(ui, |ui| {
            // nx
            ui.label("X Grid Lines:");
            let prev_nx = state.grid_config.nx;
            ui.add(
                egui::Slider::new(&mut state.grid_config.nx, 2..=20)
                    .text("")
                    .clamp_to_range(true),
            );
            if state.grid_config.nx != prev_nx {
                changed = true;
            }
            ui.end_row();

            // ny
            ui.label("Y Grid Lines:");
            let prev_ny = state.grid_config.ny;
            ui.add(
                egui::Slider::new(&mut state.grid_config.ny, 2..=100)
                    .text("")
                    .clamp_to_range(true),
            );
            if state.grid_config.ny != prev_ny {
                changed = true;
            }
            ui.end_row();

            // nz (floor levels including ground)
            ui.label("Z Levels (incl. ground):");
            let prev_nz = state.grid_config.nz;
            ui.add(
                egui::Slider::new(&mut state.grid_config.nz, 1..=20)
                    .text("")
                    .clamp_to_range(true),
            );
            if state.grid_config.nz != prev_nz {
                changed = true;
            }
            ui.end_row();

            // dx
            ui.label("X Spacing (mm):");
            let prev_dx = state.grid_config.dx;
            ui.add(
                egui::Slider::new(&mut state.grid_config.dx, 1000.0..=20000.0)
                    .text("")
                    .fixed_decimals(0)
                    .clamp_to_range(true),
            );
            if (state.grid_config.dx - prev_dx).abs() > 1e-6 {
                changed = true;
            }
            ui.end_row();

            // dy
            ui.label("Y Spacing (mm):");
            let prev_dy = state.grid_config.dy;
            ui.add(
                egui::Slider::new(&mut state.grid_config.dy, 1000.0..=20000.0)
                    .text("")
                    .fixed_decimals(0)
                    .clamp_to_range(true),
            );
            if (state.grid_config.dy - prev_dy).abs() > 1e-6 {
                changed = true;
            }
            ui.end_row();

            // dz
            ui.label("Z Interval / Floor height (mm):");
            let prev_dz = state.grid_config.dz;
            ui.add(
                egui::Slider::new(&mut state.grid_config.dz, 1000.0..=10000.0)
                    .text("")
                    .fixed_decimals(0)
                    .clamp_to_range(true),
            );
            if (state.grid_config.dz - prev_dz).abs() > 1e-6 {
                changed = true;
            }
            ui.end_row();
        });

    // Show pool stats
    ui.add_space(6.0);
    let nx = state.grid_config.nx;
    let ny = state.grid_config.ny;
    let nz = state.grid_config.nz;
    let est_nodes = nx * ny * nz;
    let est_cols = nx * ny * (nz - 1);
    let floors = nz - 1;
    let est_gdr_x = (nx - 1) * ny * floors;
    let est_gdr_y = nx * (ny - 1) * floors;
    let est_elements = est_cols + est_gdr_x + est_gdr_y;
    ui.label(
        egui::RichText::new(format!(
            "Pool estimate: {} nodes, {} elements  ({} columns + {} girders)",
            est_nodes,
            est_elements,
            est_cols,
            est_gdr_x + est_gdr_y
        ))
        .color(Color32::from_rgb(160, 220, 160)),
    );
    if est_elements > 5_000 {
        ui.label(
            egui::RichText::new(
                "Warning: Large model detected. Simulation may take longer; progress UI will help keep the app responsive.",
            )
            .color(Color32::from_rgb(255, 200, 80)),
        );
    }

    ui.add_space(12.0);
    ui.separator();

    // ── Upper-floor threshold ─────────────────────────────────────────────
    ui.heading("Construction Constraints");
    ui.add_space(4.0);
    ui.label("Upper-Floor Column Rate Threshold:");
    ui.horizontal(|ui| {
        ui.add(
            egui::Slider::new(&mut state.upper_floor_threshold, 0.0..=1.0)
                .text("")
                .fixed_decimals(2)
                .clamp_to_range(true),
        );
        ui.label(format!("{:.0}%", state.upper_floor_threshold * 100.0));
    });

    ui.add_space(12.0);
    ui.separator();

    // ── Algorithm weights ─────────────────────────────────────────────────
    ui.heading("Algorithm Weights");
    ui.add_space(4.0);
    let (ref mut w1, ref mut w2, ref mut w3) = state.sim_weights;
    egui::Grid::new("sim_weights_grid")
        .num_columns(2)
        .spacing([20.0, 6.0])
        .show(ui, |ui| {
            ui.label("w1 — Min. members (0.5):");
            ui.add(
                egui::Slider::new(w1, 0.0..=1.0)
                    .text("")
                    .fixed_decimals(2)
                    .clamp_to_range(true),
            );
            ui.end_row();

            ui.label("w2 — Connectivity (0.3):");
            ui.add(
                egui::Slider::new(w2, 0.0..=1.0)
                    .text("")
                    .fixed_decimals(2)
                    .clamp_to_range(true),
            );
            ui.end_row();

            ui.label("w3 — Distance (0.15):");
            ui.add(
                egui::Slider::new(w3, 0.0..=1.0)
                    .text("")
                    .fixed_decimals(2)
                    .clamp_to_range(true),
            );
            ui.end_row();
        });

    ui.add_space(12.0);
    ui.separator();

    // ── Scenario count ────────────────────────────────────────────────────
    ui.heading("Scenarios");
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label("Number of scenarios:");
        ui.add(
            egui::Slider::new(&mut state.sim_scenario_count, 1..=200)
                .text("")
                .clamp_to_range(true),
        );
    });

    ui.add_space(12.0);
    ui.separator();

    // ── Workfront list ────────────────────────────────────────────────────
    ui.heading("Workfronts");
    ui.add_space(4.0);
    if state.sim_workfronts.is_empty() {
        ui.colored_label(
            Color32::from_rgb(200, 180, 80),
            "No workfronts set. Switch to View tab and click grid intersections.",
        );
    } else {
        egui::ScrollArea::vertical()
            .id_source("wf_list_scroll")
            .max_height(120.0)
            .show(ui, |ui| {
                let mut to_remove: Option<usize> = None;
                for (i, wf) in state.sim_workfronts.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(format!(
                            "WF {} — Grid ({}, {})",
                            wf.id, wf.grid_x, wf.grid_y
                        ));
                        if ui.small_button("✕").clicked() {
                            to_remove = Some(i);
                        }
                    });
                }
                if let Some(idx) = to_remove {
                    state.sim_workfronts.remove(idx);
                    // Re-assign IDs (1-indexed, sequential)
                    for (i, wf) in state.sim_workfronts.iter_mut().enumerate() {
                        wf.id = (i + 1) as i32;
                    }
                }
            });
    }

    if changed {
        // Grid changed → clear workfronts (they might be out of range) and scenarios
        state.sim_workfronts.clear();
        state.sim_scenarios.clear();
        state.sim_selected_scenario = None;
    }

    changed
}

// ============================================================================
// View tab — Grid plan view + Workfront intersection selector
// ============================================================================

/// Pixel margin around the grid drawing area
const GRID_MARGIN: f32 = 40.0;

/// Radius of workfront marker circle (px)
const WF_MARKER_RADIUS: f32 = 8.0;

/// Radius of hit-test for click detection (px)
const HIT_RADIUS: f32 = 12.0;

/// Render the simulation view in the View tab.
/// Draws the x-y grid plan and handles intersection clicks to add/remove workfronts.
///
/// Returns true if a workfront was added or removed (needs repaint).
pub fn render_sim_view(ui: &mut Ui, state: &mut UiState) -> bool {
    let mut changed = false;

    ui.label("Click grid intersections to set Workfronts. Click again to remove.");
    ui.add_space(4.0);

    let available = ui.available_rect_before_wrap();

    // Allocate the full available rect as an interactive response
    let response = ui.allocate_rect(available, egui::Sense::click());
    let painter = ui.painter_at(available);

    // Background
    painter.rect_filled(available, 0.0, Color32::from_gray(18));

    let cfg = &state.grid_config;
    let nx = cfg.nx;
    let ny = cfg.ny;

    // ── Compute grid cell size to fit within available area ───────────────
    let draw_w = available.width() - 2.0 * GRID_MARGIN;
    let draw_h = available.height() - 2.0 * GRID_MARGIN;

    if draw_w < 10.0 || draw_h < 10.0 || nx < 2 || ny < 2 {
        painter.text(
            available.center(),
            egui::Align2::CENTER_CENTER,
            "Grid too small to display",
            FontId::proportional(14.0),
            Color32::from_gray(150),
        );
        return false;
    }

    let cell_w = draw_w / (nx - 1) as f32;
    let cell_h = draw_h / (ny - 1) as f32;
    let origin = Pos2::new(
        available.left() + GRID_MARGIN,
        available.top() + GRID_MARGIN,
    );

    // ── Helper: grid index → screen pos ──────────────────────────────────
    let grid_to_screen = |xi: usize, yi: usize| -> Pos2 {
        Pos2::new(origin.x + xi as f32 * cell_w, origin.y + yi as f32 * cell_h)
    };

    // ── Draw grid lines ───────────────────────────────────────────────────
    let grid_stroke = Stroke::new(1.0, Color32::from_gray(70));
    for xi in 0..nx {
        let top = grid_to_screen(xi, 0);
        let bot = grid_to_screen(xi, ny - 1);
        painter.line_segment([top, bot], grid_stroke);
    }
    for yi in 0..ny {
        let left = grid_to_screen(0, yi);
        let right = grid_to_screen(nx - 1, yi);
        painter.line_segment([left, right], grid_stroke);
    }

    // ── Draw X-axis labels (grid x indices, 0..nx-1) ─────────────────────
    let label_font = FontId::proportional(10.0);
    let label_color = Color32::from_gray(160);
    for xi in 0..nx {
        let p = grid_to_screen(xi, 0);
        painter.text(
            Pos2::new(p.x, p.y - 14.0),
            egui::Align2::CENTER_BOTTOM,
            format!("X{}", xi),
            label_font.clone(),
            label_color,
        );
    }
    for yi in 0..ny {
        let p = grid_to_screen(0, yi);
        painter.text(
            Pos2::new(p.x - 14.0, p.y),
            egui::Align2::RIGHT_CENTER,
            format!("Y{}", yi),
            label_font.clone(),
            label_color,
        );
    }

    // ── Draw existing workfront markers ───────────────────────────────────
    let wf_colors = [
        Color32::from_rgb(100, 200, 255),
        Color32::from_rgb(100, 255, 150),
        Color32::from_rgb(255, 200, 100),
        Color32::from_rgb(255, 120, 200),
        Color32::from_rgb(180, 130, 255),
        Color32::from_rgb(255, 255, 100),
    ];
    for wf in &state.sim_workfronts {
        if wf.grid_x < nx && wf.grid_y < ny {
            let p = grid_to_screen(wf.grid_x, wf.grid_y);
            let color = wf_colors[(wf.id as usize - 1) % wf_colors.len()];
            painter.circle_filled(p, WF_MARKER_RADIUS, color);
            painter.circle_stroke(p, WF_MARKER_RADIUS, Stroke::new(1.5, Color32::WHITE));
            painter.text(
                Pos2::new(p.x, p.y - WF_MARKER_RADIUS - 4.0),
                egui::Align2::CENTER_BOTTOM,
                format!("WF{}", wf.id),
                FontId::proportional(9.0),
                color,
            );
        }
    }

    // ── Draw intersection dots for unoccupied positions ───────────────────
    for xi in 0..nx {
        for yi in 0..ny {
            let occupied = state
                .sim_workfronts
                .iter()
                .any(|wf| wf.grid_x == xi && wf.grid_y == yi);
            if !occupied {
                let p = grid_to_screen(xi, yi);
                painter.circle_filled(p, 3.0, Color32::from_gray(120));
            }
        }
    }

    // ── Handle click ──────────────────────────────────────────────────────
    if response.clicked() {
        if let Some(click_pos) = response.interact_pointer_pos() {
            // Find nearest grid intersection within HIT_RADIUS
            let mut best: Option<(usize, usize, f32)> = None;
            for xi in 0..nx {
                for yi in 0..ny {
                    let p = grid_to_screen(xi, yi);
                    let dist = ((p.x - click_pos.x).powi(2) + (p.y - click_pos.y).powi(2)).sqrt();
                    if dist <= HIT_RADIUS {
                        if best.as_ref().map_or(true, |(_, _, d)| dist < *d) {
                            best = Some((xi, yi, dist));
                        }
                    }
                }
            }

            if let Some((xi, yi, _)) = best {
                // Toggle: if already set → remove, else → add
                if let Some(pos) = state
                    .sim_workfronts
                    .iter()
                    .position(|wf| wf.grid_x == xi && wf.grid_y == yi)
                {
                    state.sim_workfronts.remove(pos);
                    // Re-assign IDs
                    for (i, wf) in state.sim_workfronts.iter_mut().enumerate() {
                        wf.id = (i + 1) as i32;
                    }
                } else {
                    let new_id = state.sim_workfronts.len() as i32 + 1; // 1-indexed
                    state.sim_workfronts.push(SimWorkfront {
                        id: new_id,
                        grid_x: xi,
                        grid_y: yi,
                    });
                }
                changed = true;
            }
        }
    }

    // ── Info overlay ──────────────────────────────────────────────────────
    let info_rect = Rect::from_min_size(
        Pos2::new(available.left() + 4.0, available.bottom() - 22.0),
        egui::Vec2::new(available.width() - 8.0, 18.0),
    );
    painter.text(
        info_rect.left_center(),
        egui::Align2::LEFT_CENTER,
        format!(
            "Grid {}×{}  |  {} floors  |  {} workfront(s)",
            nx,
            ny,
            cfg.nz.saturating_sub(1),
            state.sim_workfronts.len()
        ),
        FontId::proportional(10.0),
        Color32::from_gray(140),
    );

    changed
}

// ============================================================================
// Scenario result panel (Result tab, Simulation mode)
// ============================================================================

/// Render the simulation result tab.
/// Shows scenario list, playback controls, selected scenario metrics.
pub fn render_sim_result(ui: &mut Ui, state: &mut UiState) {
    ui.heading("Simulation Results");
    ui.separator();

    if state.sim_running {
        ui.colored_label(Color32::from_rgb(100, 200, 255), "⏳ Simulation running…");
        return;
    }

    if state.sim_scenarios.is_empty() {
        ui.colored_label(
            Color32::from_rgb(200, 180, 80),
            "No scenarios generated yet. Configure Grid, set Workfronts, then press Recalc.",
        );
        return;
    }

    // ── Scenario list ──────────────────────────────────────────────────────
    ui.heading("Scenarios");
    ui.add_space(4.0);

    egui::ScrollArea::vertical()
        .id_source("scenario_list_scroll")
        .max_height(160.0)
        .show(ui, |ui| {
            egui::Grid::new("scenario_grid")
                .num_columns(5)
                .spacing([12.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("#");
                    ui.strong("Steps");
                    ui.strong("Members");
                    ui.strong("Avg/Step");
                    ui.strong("Termination");
                    ui.end_row();

                    for scenario in &state.sim_scenarios {
                        let selected = state
                            .sim_selected_scenario
                            .map_or(false, |s| s == scenario.id - 1);
                        let row_color = if selected {
                            Color32::from_rgb(50, 80, 120)
                        } else {
                            Color32::TRANSPARENT
                        };
                        // Highlight row background with a colored label on the ID
                        let id_label = egui::RichText::new(format!("{}", scenario.id))
                            .color(if selected {
                                Color32::from_rgb(100, 200, 255)
                            } else {
                                Color32::from_gray(200)
                            })
                            .strong();
                        let _ = row_color; // used implicitly via RichText

                        if ui.label(id_label).clicked()
                            || ui
                                .selectable_label(
                                    selected,
                                    format!("{}", scenario.metrics.total_steps),
                                )
                                .clicked()
                        {
                            state.sim_selected_scenario = Some(scenario.id - 1);
                            state.sim_current_step = 1;
                            state.sim_playing = false;
                        }
                        ui.label(format!("{}", scenario.metrics.total_members_installed));
                        ui.label(format!("{:.2}", scenario.metrics.avg_members_per_step));
                        ui.label(format!("{}", scenario.metrics.termination_reason));
                        ui.end_row();
                    }
                });
        });

    ui.add_space(10.0);
    ui.separator();

    // ── Playback controls ──────────────────────────────────────────────────
    if let Some(sel_idx) = state.sim_selected_scenario {
        if let Some(scenario) = state.sim_scenarios.get(sel_idx) {
            let max_step = scenario.steps.len();

            ui.heading(format!("Scenario {} — Playback", scenario.id));
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                // Prev step
                if ui
                    .add_enabled(state.sim_current_step > 1, egui::Button::new("◀"))
                    .clicked()
                {
                    state.sim_current_step = state.sim_current_step.saturating_sub(1).max(1);
                    state.sim_playing = false;
                }

                // Play / Pause
                if state.sim_playing {
                    if ui.button("⏸ Pause").clicked() {
                        state.sim_playing = false;
                    }
                } else if ui
                    .add_enabled(
                        max_step > 0 && state.sim_current_step < max_step,
                        egui::Button::new("▶ Play"),
                    )
                    .clicked()
                {
                    state.sim_playing = true;
                }

                // Next step
                if ui
                    .add_enabled(state.sim_current_step < max_step, egui::Button::new("▶"))
                    .clicked()
                {
                    state.sim_current_step = (state.sim_current_step + 1).min(max_step);
                    state.sim_playing = false;
                }

                // Speed selector
                ui.separator();
                ui.label("Speed:");
                ui.selectable_value(&mut state.sim_speed, 1, "1×");
                ui.selectable_value(&mut state.sim_speed, 2, "2×");
                ui.selectable_value(&mut state.sim_speed, 4, "4×");
            });

            // Seek bar (slider)
            if max_step > 0 {
                ui.add(
                    egui::Slider::new(&mut state.sim_current_step, 1..=max_step)
                        .text("Step")
                        .clamp_to_range(true),
                );
            }

            // Step info
            let step_display = state.sim_current_step.min(max_step).max(1);
            if let Some(step) = scenario.steps.get(step_display - 1) {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(format!("Step {} / {}", step_display, max_step));
                    ui.separator();
                    if step.local_steps.len() > 1 {
                        let wf_detail = step.local_steps.iter()
                            .map(|ls| format!("WF {}: {} ({})", ls.workfront_id, ls.element_ids.len(), ls.pattern))
                            .collect::<Vec<_>>()
                            .join(", ");
                        ui.label(format!("[{}]", wf_detail));
                    } else {
                        ui.label(format!("WF {}", step.workfront_id));
                        ui.separator();
                        ui.label(format!("Floor {}", step.floor));
                    }
                    ui.separator();
                    ui.label(format!(
                        "{} member(s): {:?}",
                        step.element_ids.len(),
                        step.element_ids
                    ));
                });
            }

            ui.add_space(10.0);
            ui.separator();

            // ── Metrics summary ────────────────────────────────────────────
            ui.heading("Metrics");
            ui.add_space(4.0);
            egui::Grid::new("sim_metrics_grid")
                .num_columns(2)
                .spacing([20.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Total Steps:");
                    ui.label(format!("{}", scenario.metrics.total_steps));
                    ui.end_row();

                    ui.label("Members Installed:");
                    ui.label(format!("{}", scenario.metrics.total_members_installed));
                    ui.end_row();

                    ui.label("Avg Members/Step:");
                    ui.label(format!("{:.2}", scenario.metrics.avg_members_per_step));
                    ui.end_row();

                    ui.label("Avg Connectivity:");
                    ui.label(format!("{:.2}", scenario.metrics.avg_connectivity));
                    ui.end_row();

                    ui.label("Termination:");
                    ui.label(format!("{}", scenario.metrics.termination_reason));
                    ui.end_row();
                });

            ui.add_space(10.0);
            ui.separator();

            // ── XY Plot: Members-per-step ──────────────────────────────────
            render_members_per_step_plot(ui, scenario);

            // ── Floor chart (Dev Chart 2 equivalent) ───────────────────────
            render_floor_column_installation_rate_chart(ui, state, scenario);

            // ── Upper-floor ratio chart (Dev Chart 3 equivalent) ───────────
            render_upper_floor_column_rate_chart(ui, state, scenario);

            // ── Scenario comparison chart ──────────────────────────────────
            render_scenario_comparison_chart(ui, state);

            ui.add_space(10.0);
            ui.separator();

            // ── Export ────────────────────────────────────────────────────
            ui.heading("Export Debug Files");
            ui.add_space(4.0);
            ui.label(
                "Save scenario steps and summary to CSV/text files in the same folder as the executable.",
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !state.sim_export_requested,
                        egui::Button::new("💾 Export All Scenarios"),
                    )
                    .on_hover_text(
                        "Export all scenario steps + summary to simulation_debug/ folder",
                    )
                    .clicked()
                {
                    state.sim_export_requested = true;
                    state.sim_export_status = "Exporting…".to_string();
                }
                if ui
                    .add_enabled(
                        !state.sim_export_requested && state.sim_selected_scenario.is_some(),
                        egui::Button::new("💾 Export Selected Scenario"),
                    )
                    .on_hover_text("Export only the currently selected scenario")
                    .clicked()
                {
                    // Use negative index trick: store selected-only flag via a sentinel
                    // We encode "export selected only" as usize::MAX sentinel in sim_current_step
                    // Actually: simpler — use a dedicated field. We reuse sim_export_requested
                    // and pass selected info through sim_export_status prefix.
                    state.sim_export_requested = true;
                    let sel_id = state
                        .sim_selected_scenario
                        .and_then(|i| state.sim_scenarios.get(i))
                        .map(|s| s.id)
                        .unwrap_or(0);
                    state.sim_export_status = format!("Exporting scenario {}…", sel_id);
                }
            });
            // Show last export result
            if !state.sim_export_status.is_empty() {
                ui.add_space(4.0);
                let color = if state.sim_export_status.starts_with("✅") {
                    Color32::from_rgb(80, 200, 120)
                } else if state.sim_export_status.starts_with("❌") {
                    Color32::from_rgb(220, 80, 80)
                } else {
                    Color32::from_rgb(180, 180, 80)
                };
                ui.colored_label(color, &state.sim_export_status);
            }
        }
    }
}

/// Draw a line chart of members-installed per step for a given scenario.
/// X = step index (1-indexed), Y = member count at that step.
/// Shows a target band (1.8~2.4) and a mean line.
fn render_members_per_step_plot(ui: &mut Ui, scenario: &crate::graphics::ui::SimScenario) {
    use eframe::egui::pos2;

    let steps = &scenario.steps;
    if steps.is_empty() {
        return;
    }

    ui.heading("Members per Step");
    ui.add_space(4.0);

    // Desired plot dimensions
    let plot_h = 150.0f32;

    egui::Frame::none()
        .inner_margin(egui::Margin::symmetric(CHART_MARGIN_H, 0.0))
        .show(ui, |ui| {
            let (chart_rect, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), plot_h),
                egui::Sense::hover(),
            );
            let painter = ui.painter_at(chart_rect);
            painter.rect_filled(chart_rect, 2.0, Color32::from_rgb(30, 30, 40));

            let plot_rect = Rect::from_min_max(
                pos2(
                    chart_rect.left() + CHART_PADDING_LEFT,
                    chart_rect.top() + CHART_PADDING_TOP,
                ),
                pos2(
                    chart_rect.right() - CHART_PADDING_RIGHT - CHART_LEGEND_W,
                    chart_rect.bottom() - CHART_PADDING_BOTTOM,
                ),
            );

            painter.rect_stroke(plot_rect, 2.0, Stroke::new(1.0, Color32::from_gray(60)));

            let n = steps.len();
            let counts: Vec<usize> = steps.iter().map(|s| s.element_ids.len()).collect();
            let max_count = counts.iter().copied().max().unwrap_or(1).max(1);
            let y_max = (max_count as f32 * 1.25).max(5.0);

    // Helper: data coords → screen coords
            let to_screen = |step_i: f32, count: f32| -> Pos2 {
                let x = plot_rect.left() + (step_i / (n as f32).max(1.0)) * plot_rect.width();
                let y = plot_rect.bottom() - (count / y_max) * plot_rect.height();
                pos2(x, y)
            };

    // ── Target band (1.8 ~ 2.4) ───────────────────────────────────────────
            let y_band_lo = plot_rect.bottom() - (1.8 / y_max) * plot_rect.height();
            let y_band_hi = plot_rect.bottom() - (2.4 / y_max) * plot_rect.height();
            let band_rect = Rect::from_min_max(
                pos2(plot_rect.left(), y_band_hi),
                pos2(plot_rect.right(), y_band_lo),
            );
            painter.rect_filled(
                band_rect,
                0.0,
                Color32::from_rgba_unmultiplied(80, 200, 80, 28),
            );

    // ── Y-axis grid lines & labels (0, y_max/2, y_max) ───────────────────
            let y_ticks = [0.0f32, y_max * 0.5, y_max];
            for &yv in &y_ticks {
                let sy = plot_rect.bottom() - (yv / y_max) * plot_rect.height();
                let dash = 5.0f32;
                let gap = 4.0f32;
                let mut x = plot_rect.left();
                while x < plot_rect.right() {
                    let x_end = (x + dash).min(plot_rect.right());
                    painter.line_segment(
                        [pos2(x, sy), pos2(x_end, sy)],
                        Stroke::new(0.5, Color32::from_gray(50)),
                    );
                    x += dash + gap;
                }
                painter.text(
                    pos2(plot_rect.left() - 4.0, sy),
                    egui::Align2::RIGHT_CENTER,
                    format!("{:.0}", yv),
                    FontId::proportional(9.0),
                    Color32::from_gray(140),
                );
            }

    // ── Data line ─────────────────────────────────────────────────────────
            let line_color = Color32::from_rgb(100, 180, 255);
            let mut points: Vec<Pos2> = Vec::with_capacity(n);
            for (i, &count) in counts.iter().enumerate() {
                let xi = (i as f32 + 0.5) / n as f32 * n as f32;
                points.push(to_screen(xi, count as f32));
            }

            for w in points.windows(2) {
                painter.line_segment([w[0], w[1]], Stroke::new(1.5, line_color));
            }
            for &p in &points {
                painter.circle_filled(p, 2.5, line_color);
            }

    // ── Mean line (red dashed) ────────────────────────────────────────────
            let mean = scenario.metrics.avg_members_per_step as f32;
            let y_mean = plot_rect.bottom() - (mean / y_max) * plot_rect.height();
            let dash = 6.0f32;
            let gap = 4.0f32;
            let mut x = plot_rect.left();
            while x < plot_rect.right() {
                let x_end = (x + dash).min(plot_rect.right());
                painter.line_segment(
                    [pos2(x, y_mean), pos2(x_end, y_mean)],
                    Stroke::new(1.5, Color32::from_rgb(255, 100, 100)),
                );
                x += dash + gap;
            }
            painter.text(
                pos2(plot_rect.right() + 2.0, y_mean),
                egui::Align2::LEFT_CENTER,
                format!("avg {:.1}", mean),
                FontId::proportional(9.0),
                Color32::from_rgb(255, 120, 120),
            );

    // ── X-axis label ──────────────────────────────────────────────────────
            painter.text(
                pos2(plot_rect.center_top().x, chart_rect.bottom() - 2.0),
                egui::Align2::CENTER_BOTTOM,
                "Step",
                FontId::proportional(9.0),
                Color32::from_gray(140),
            );

    // ── Legend ────────────────────────────────────────────────────────────
            let legend_x = plot_rect.right() + 6.0;
            let legend_y = plot_rect.top() + 6.0;
            painter.line_segment(
                [
                    pos2(legend_x, legend_y + 4.0),
                    pos2(legend_x + 14.0, legend_y + 4.0),
                ],
                Stroke::new(1.5, line_color),
            );
            painter.text(
                pos2(legend_x + 16.0, legend_y + 4.0),
                egui::Align2::LEFT_CENTER,
                "members/step",
                FontId::proportional(8.0),
                Color32::from_gray(180),
            );
            let legend_y2 = legend_y + 14.0;
            painter.rect_filled(
                Rect::from_min_size(pos2(legend_x, legend_y2), Vec2::new(14.0, 6.0)),
                0.0,
                Color32::from_rgba_unmultiplied(80, 200, 80, 60),
            );
            painter.text(
                pos2(legend_x + 16.0, legend_y2 + 3.0),
                egui::Align2::LEFT_CENTER,
                "target 1.8~2.4",
                FontId::proportional(8.0),
                Color32::from_gray(160),
            );

            ui.add_space(8.0);
        });
}

fn render_floor_column_installation_rate_chart(
    ui: &mut Ui,
    state: &UiState,
    scenario: &crate::graphics::ui::SimScenario,
) {
    use eframe::egui::pos2;

    let floors = state.grid_config.nz.saturating_sub(1);
    if floors == 0 || scenario.steps.is_empty() {
        return;
    }

    ui.add_space(10.0);
    ui.separator();
    ui.heading("Floor Column Installation Rate by Step");
    ui.add_space(4.0);

    let columns_per_floor = state.grid_config.nx * state.grid_config.ny;
    let total_columns = columns_per_floor * floors;
    let n_steps = scenario.steps.len();

    // In SimGrid, column element IDs come first in ascending order.
    let is_column = |eid: i32| -> bool { eid >= 1 && (eid as usize) <= total_columns };

    let mut floor_rates: Vec<Vec<f32>> = vec![Vec::with_capacity(n_steps); floors];
    let mut cum_cols_by_floor: Vec<usize> = vec![0; floors];

    for step in &scenario.steps {
        for local_step in &step.local_steps {
            let floor_idx = (local_step.floor.saturating_sub(1) as usize).min(floors - 1);
            let cols_in_local = local_step
                .element_ids
                .iter()
                .filter(|&&eid| is_column(eid))
                .count();
            cum_cols_by_floor[floor_idx] += cols_in_local;
        }

        for floor_idx in 0..floors {
            let rate = (cum_cols_by_floor[floor_idx] as f32 / columns_per_floor.max(1) as f32)
                .min(1.0);
            floor_rates[floor_idx].push(rate);
        }
    }

    let floor_colors = [
        Color32::from_rgb(255, 100, 100),
        Color32::from_rgb(255, 180, 60),
        Color32::from_rgb(100, 220, 100),
        Color32::from_rgb(80, 180, 255),
        Color32::from_rgb(200, 100, 255),
        Color32::from_rgb(255, 120, 200),
        Color32::from_rgb(100, 240, 220),
        Color32::from_rgb(255, 240, 100),
    ];

    let chart_h = 180.0f32;
    egui::Frame::none()
        .inner_margin(egui::Margin::symmetric(CHART_MARGIN_H, 0.0))
        .show(ui, |ui| {
            let (chart_rect, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), chart_h),
                egui::Sense::hover(),
            );
            let painter = ui.painter_at(chart_rect);
            painter.rect_filled(chart_rect, 2.0, Color32::from_rgb(30, 30, 40));

            let plot_rect = Rect::from_min_max(
                pos2(
                    chart_rect.left() + CHART_PADDING_LEFT,
                    chart_rect.top() + CHART_PADDING_TOP,
                ),
                pos2(
                    chart_rect.right() - CHART_PADDING_RIGHT - CHART_LEGEND_W,
                    chart_rect.bottom() - CHART_PADDING_BOTTOM,
                ),
            );

            let axis_color = Color32::from_rgb(160, 160, 160);
            let grid_color = Color32::from_rgb(60, 60, 70);
            let font_id = FontId::proportional(10.0);

            painter.line_segment(
                [plot_rect.left_bottom(), plot_rect.left_top()],
                Stroke::new(1.0, axis_color),
            );
            painter.line_segment(
                [plot_rect.left_bottom(), plot_rect.right_bottom()],
                Stroke::new(1.0, axis_color),
            );

            for (frac, label) in &[(0.0f32, "0%"), (0.5, "50%"), (1.0, "100%")] {
                let y = plot_rect.bottom() - frac * plot_rect.height();
                painter.line_segment(
                    [pos2(plot_rect.left(), y), pos2(plot_rect.right(), y)],
                    Stroke::new(if *frac == 1.0 { 0.8 } else { 0.5 }, grid_color),
                );
                painter.text(
                    pos2(chart_rect.left(), y),
                    egui::Align2::LEFT_CENTER,
                    *label,
                    font_id.clone(),
                    axis_color,
                );
            }

            let max_step_f = n_steps as f32;
            let to_screen = |step: f32, rate: f32| -> Pos2 {
                let x = plot_rect.left()
                    + (step - 1.0) / (max_step_f - 1.0).max(1.0) * plot_rect.width();
                let y = plot_rect.bottom() - rate * plot_rect.height();
                pos2(x, y)
            };

            for (fi, rates) in floor_rates.iter().enumerate() {
                let color = floor_colors[fi % floor_colors.len()];
                if rates.len() >= 2 {
                    let points: Vec<Pos2> = rates
                        .iter()
                        .enumerate()
                        .map(|(i, rate)| to_screen(i as f32 + 1.0, *rate))
                        .collect();
                    painter.add(egui::Shape::line(points, Stroke::new(1.5, color)));
                }

                let legend_x = plot_rect.right() + 6.0;
                let legend_y = chart_rect.top() + CHART_PADDING_TOP + fi as f32 * 14.0;
                painter.line_segment(
                    [pos2(legend_x, legend_y + 5.0), pos2(legend_x + 14.0, legend_y + 5.0)],
                    Stroke::new(2.0, color),
                );
                painter.text(
                    pos2(legend_x + 17.0, legend_y + 5.0),
                    egui::Align2::LEFT_CENTER,
                    format!("F{}", fi + 1),
                    font_id.clone(),
                    color,
                );
            }

            let label_step = (n_steps / 10).max(1);
            for s in (1..=n_steps).step_by(label_step) {
                let x = plot_rect.left()
                    + (s as f32 - 1.0) / (max_step_f - 1.0).max(1.0) * plot_rect.width();
                painter.text(
                    pos2(x, chart_rect.bottom() - 4.0),
                    egui::Align2::CENTER_BOTTOM,
                    format!("{}", s),
                    font_id.clone(),
                    axis_color,
                );
            }
        });
}

fn render_upper_floor_column_rate_chart(
    ui: &mut Ui,
    state: &UiState,
    scenario: &crate::graphics::ui::SimScenario,
) {
    use eframe::egui::pos2;

    let floors = state.grid_config.nz.saturating_sub(1);
    if floors < 2 || scenario.steps.is_empty() {
        return;
    }

    ui.add_space(10.0);
    ui.separator();
    ui.heading("Upper-Floor Column Installation Rate");
    ui.add_space(3.0);
    ui.label(
        egui::RichText::new(format!(
            "Threshold: {:.0}%  —  each line F{{N+1}}/F{{N}}",
            state.upper_floor_threshold * 100.0
        ))
        .weak()
        .small(),
    );
    ui.add_space(4.0);

    let columns_per_floor = state.grid_config.nx * state.grid_config.ny;
    let total_columns = columns_per_floor * floors;
    let n_steps = scenario.steps.len();
    let lower_floor_count = floors - 1;
    let is_column = |eid: i32| -> bool { eid >= 1 && (eid as usize) <= total_columns };

    let mut ratios: Vec<Vec<f32>> = vec![Vec::with_capacity(n_steps); lower_floor_count];
    let mut cum_cols_by_floor: Vec<usize> = vec![0; floors];

    for step in &scenario.steps {
        for local_step in &step.local_steps {
            let floor_idx = (local_step.floor.saturating_sub(1) as usize).min(floors - 1);
            let cols_in_local = local_step
                .element_ids
                .iter()
                .filter(|&&eid| is_column(eid))
                .count();
            cum_cols_by_floor[floor_idx] += cols_in_local;
        }

        for floor_idx in 0..lower_floor_count {
            let lower = cum_cols_by_floor[floor_idx] as f32;
            let upper = cum_cols_by_floor[floor_idx + 1] as f32;
            let ratio = if lower <= 0.0 { 0.0 } else { upper / lower };
            ratios[floor_idx].push(ratio);
        }
    }

    let y_max = ratios
        .iter()
        .flat_map(|series| series.iter().copied())
        .fold(1.0f32, f32::max)
        .max(1.0);

    let floor_colors = [
        Color32::from_rgb(255, 100, 100),
        Color32::from_rgb(255, 180, 60),
        Color32::from_rgb(100, 220, 100),
        Color32::from_rgb(80, 180, 255),
        Color32::from_rgb(200, 100, 255),
        Color32::from_rgb(255, 120, 200),
        Color32::from_rgb(100, 240, 220),
        Color32::from_rgb(255, 240, 100),
    ];

    let chart_h = 180.0f32;
    egui::Frame::none()
        .inner_margin(egui::Margin::symmetric(CHART_MARGIN_H, 0.0))
        .show(ui, |ui| {
            let (chart_rect, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), chart_h),
                egui::Sense::hover(),
            );
            let painter = ui.painter_at(chart_rect);
            painter.rect_filled(chart_rect, 2.0, Color32::from_rgb(30, 30, 40));

            let plot_rect = Rect::from_min_max(
                pos2(
                    chart_rect.left() + CHART_PADDING_LEFT,
                    chart_rect.top() + CHART_PADDING_TOP,
                ),
                pos2(
                    chart_rect.right() - CHART_PADDING_RIGHT - CHART_LEGEND_W,
                    chart_rect.bottom() - CHART_PADDING_BOTTOM,
                ),
            );

            let axis_color = Color32::from_rgb(160, 160, 160);
            let grid_color = Color32::from_rgb(60, 60, 70);
            let font_id = FontId::proportional(10.0);

            painter.line_segment(
                [plot_rect.left_bottom(), plot_rect.left_top()],
                Stroke::new(1.0, axis_color),
            );
            painter.line_segment(
                [plot_rect.left_bottom(), plot_rect.right_bottom()],
                Stroke::new(1.0, axis_color),
            );

            for (frac, label) in &[
                (0.0f32, "0".to_string()),
                (0.5f32, format!("{:.1}", y_max * 0.5)),
                (1.0f32, format!("{:.1}", y_max)),
            ] {
                let y = plot_rect.bottom() - frac * plot_rect.height();
                painter.line_segment(
                    [pos2(plot_rect.left(), y), pos2(plot_rect.right(), y)],
                    Stroke::new(if *frac == 1.0 { 0.8 } else { 0.5 }, grid_color),
                );
                painter.text(
                    pos2(chart_rect.left(), y),
                    egui::Align2::LEFT_CENTER,
                    label,
                    font_id.clone(),
                    axis_color,
                );
            }

            let max_step_f = n_steps as f32;
            let to_screen = |step: f32, ratio: f32| -> Pos2 {
                let x = plot_rect.left()
                    + (step - 1.0) / (max_step_f - 1.0).max(1.0) * plot_rect.width();
                let y = plot_rect.bottom() - (ratio / y_max) * plot_rect.height();
                pos2(x, y)
            };

            // Threshold line (0~1 scale on ratio axis)
            let threshold_ratio = state.upper_floor_threshold as f32;
            let threshold_y = plot_rect.bottom() - (threshold_ratio / y_max) * plot_rect.height();
            let mut x = plot_rect.left();
            while x < plot_rect.right() {
                let x_end = (x + 6.0).min(plot_rect.right());
                painter.line_segment(
                    [pos2(x, threshold_y), pos2(x_end, threshold_y)],
                    Stroke::new(1.5, Color32::from_rgb(255, 80, 80)),
                );
                x += 10.0;
            }
            painter.text(
                pos2(plot_rect.right() + 3.0, threshold_y),
                egui::Align2::LEFT_CENTER,
                format!("{:.0}%", state.upper_floor_threshold * 100.0),
                font_id.clone(),
                Color32::from_rgb(255, 80, 80),
            );

            for (fi, series) in ratios.iter().enumerate() {
                let color = floor_colors[fi % floor_colors.len()];
                if series.len() >= 2 {
                    let points: Vec<Pos2> = series
                        .iter()
                        .enumerate()
                        .map(|(i, ratio)| to_screen(i as f32 + 1.0, *ratio))
                        .collect();
                    painter.add(egui::Shape::line(points, Stroke::new(1.5, color)));
                }

                let legend_x = plot_rect.right() + 6.0;
                let legend_y = chart_rect.top() + CHART_PADDING_TOP + fi as f32 * 14.0;
                painter.line_segment(
                    [pos2(legend_x, legend_y + 5.0), pos2(legend_x + 14.0, legend_y + 5.0)],
                    Stroke::new(2.0, color),
                );
                painter.text(
                    pos2(legend_x + 17.0, legend_y + 5.0),
                    egui::Align2::LEFT_CENTER,
                    format!("F{}/F{}", fi + 2, fi + 1),
                    font_id.clone(),
                    color,
                );
            }

            let label_step = (n_steps / 10).max(1);
            for s in (1..=n_steps).step_by(label_step) {
                let x = plot_rect.left()
                    + (s as f32 - 1.0) / (max_step_f - 1.0).max(1.0) * plot_rect.width();
                painter.text(
                    pos2(x, chart_rect.bottom() - 4.0),
                    egui::Align2::CENTER_BOTTOM,
                    format!("{}", s),
                    font_id.clone(),
                    axis_color,
                );
            }
        });
}

// ============================================================================
// Scenario comparison chart (top-N multi-line cumulative members vs step)
// ============================================================================

/// Draw a multi-line XY plot comparing the top-N scenarios by cumulative
/// members installed vs construction step.
///
/// Called at the bottom of `render_sim_result` (after the single-scenario plot).
pub fn render_scenario_comparison_chart(ui: &mut Ui, state: &UiState) {
    use eframe::egui::pos2;

    use crate::graphics::ui::SimScenario;

    let scenarios = &state.sim_scenarios;
    if scenarios.len() < 2 {
        return; // comparison only meaningful with ≥ 2 scenarios
    }

    ui.add_space(10.0);
    ui.separator();
    ui.heading("Scenario Comparison (Top 10)");
    ui.add_space(4.0);

    // ── Build sorted top-N list ───────────────────────────────────────────
    let top_n = 10usize;
    let mut sorted_refs: Vec<&SimScenario> = scenarios.iter().collect();
    sorted_refs.sort_by(|a, b| {
        b.metrics
            .total_members_installed
            .cmp(&a.metrics.total_members_installed)
    });
    sorted_refs.truncate(top_n);

    // Max values for axis scaling
    let max_steps = sorted_refs
        .iter()
        .map(|s| s.steps.len())
        .max()
        .unwrap_or(1)
        .max(1);
    let max_members = sorted_refs
        .iter()
        .map(|s| s.metrics.total_members_installed)
        .max()
        .unwrap_or(1)
        .max(1);

    // Cumulative series: cum_data[i] = Vec<(step_1indexed_f32, cum_members_f32)>
    let cum_data: Vec<Vec<(f32, f32)>> = sorted_refs
        .iter()
        .map(|s| {
            let mut cum: usize = 0;
            s.steps
                .iter()
                .enumerate()
                .map(|(i, step)| {
                    cum += step.element_ids.len();
                    ((i + 1) as f32, cum as f32)
                })
                .collect()
        })
        .collect();

    // ── Layout ────────────────────────────────────────────────────────────
    let plot_h = 170.0f32;
    egui::Frame::none()
        .inner_margin(egui::Margin::symmetric(CHART_MARGIN_H, 0.0))
        .show(ui, |ui| {
            let (total_rect, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), plot_h),
                egui::Sense::hover(),
            );
            let painter = ui.painter_at(total_rect);
            painter.rect_filled(total_rect, 2.0, Color32::from_rgb(30, 30, 40));

            let plot_rect = Rect::from_min_max(
                pos2(
                    total_rect.left() + CHART_PADDING_LEFT,
                    total_rect.top() + CHART_PADDING_TOP,
                ),
                pos2(
                    total_rect.right() - CHART_PADDING_RIGHT - CHART_LEGEND_W,
                    total_rect.bottom() - CHART_PADDING_BOTTOM,
                ),
            );

            // Axis lines
            let axis_color = Color32::from_gray(120);
            painter.line_segment(
                [plot_rect.left_bottom(), plot_rect.left_top()],
                Stroke::new(1.0, axis_color),
            );
            painter.line_segment(
                [plot_rect.left_bottom(), plot_rect.right_bottom()],
                Stroke::new(1.0, axis_color),
            );

    // ── Y-axis tick labels (0, 50%, 100%) ────────────────────────────────
            let font_id = FontId::proportional(9.0);
            for frac in [0.0f32, 0.5, 1.0] {
                let val = frac * max_members as f32;
                let sy = plot_rect.bottom() - frac * plot_rect.height();
                let dash = 5.0f32;
                let gap = 4.0f32;
                let mut x = plot_rect.left();
                while x < plot_rect.right() {
                    let x_end = (x + dash).min(plot_rect.right());
                    painter.line_segment(
                        [pos2(x, sy), pos2(x_end, sy)],
                        Stroke::new(0.4, Color32::from_gray(45)),
                    );
                    x += dash + gap;
                }
                painter.text(
                    pos2(plot_rect.left() - 4.0, sy),
                    egui::Align2::RIGHT_CENTER,
                    format!("{:.0}", val),
                    font_id.clone(),
                    Color32::from_gray(130),
                );
            }

    // ── Data → screen coordinate helper ───────────────────────────────────
            let to_screen = |step: f32, members: f32| -> Pos2 {
                let x = plot_rect.left()
                    + (step - 1.0) / (max_steps as f32 - 1.0).max(1.0) * plot_rect.width();
                let y = plot_rect.bottom() - (members / max_members as f32) * plot_rect.height();
                pos2(x, y)
            };

    // ── Per-scenario line colors ──────────────────────────────────────────
            let line_colors: [Color32; 10] = [
        Color32::from_rgb(100, 200, 255), // sky blue
        Color32::from_rgb(100, 255, 150), // mint green
        Color32::from_rgb(255, 200, 80),  // amber
        Color32::from_rgb(255, 110, 190), // pink
        Color32::from_rgb(180, 130, 255), // lavender
        Color32::from_rgb(255, 255, 100), // yellow
        Color32::from_rgb(255, 100, 100), // red
        Color32::from_rgb(80, 240, 220),  // teal
        Color32::from_rgb(200, 180, 255), // lilac
        Color32::from_rgb(255, 160, 80),  // orange
            ];

    // ── Draw lines ────────────────────────────────────────────────────────
            for (rank, (scenario, series)) in sorted_refs.iter().zip(cum_data.iter()).enumerate() {
                let color = line_colors[rank % line_colors.len()];
                if series.len() >= 2 {
                    let points: Vec<Pos2> = series.iter().map(|&(s, m)| to_screen(s, m)).collect();
                    painter.add(egui::Shape::line(points, Stroke::new(1.5, color)));
                } else if series.len() == 1 {
                    let p = to_screen(series[0].0, series[0].1);
                    painter.circle_filled(p, 2.0, color);
                }

                let lx = plot_rect.right() + 6.0;
                let ly = total_rect.top() + CHART_PADDING_TOP + rank as f32 * 14.0;
                if ly + 10.0 < total_rect.bottom() {
                    painter.line_segment(
                        [pos2(lx, ly + 4.0), pos2(lx + 12.0, ly + 4.0)],
                        Stroke::new(1.5, color),
                    );
                    painter.text(
                        pos2(lx + 14.0, ly + 4.0),
                        egui::Align2::LEFT_CENTER,
                        format!("#{}", scenario.id),
                        font_id.clone(),
                        color,
                    );
                }
            }

    // ── Axis labels ───────────────────────────────────────────────────────
            painter.text(
                pos2(plot_rect.center().x, total_rect.bottom() - 2.0),
                egui::Align2::CENTER_BOTTOM,
                "Step",
                font_id.clone(),
                Color32::from_gray(130),
            );

    // Y-axis label (rotated text not available in egui; place abbreviated label at top)
            painter.text(
                pos2(total_rect.left() + 2.0, plot_rect.top()),
                egui::Align2::LEFT_TOP,
                "Members",
                font_id,
                Color32::from_gray(130),
            );
        });
}
