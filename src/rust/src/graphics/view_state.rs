use eframe::egui;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    XY,
    YZ,
    ZX,
    Orbit3D, // Isometric-style 3D view
}

#[derive(Clone, Debug)]
pub struct ViewState {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub pan_offset: egui::Vec2,
    pub zoom: f32,
    pub view_mode: ViewMode,
}

impl ViewState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Project 3D point to 2D (orthographic projection for rendering)
    pub fn project_3d_to_2d(&self, point: [f32; 3], _scene_depth: f32) -> (egui::Vec2, f32) {
        let (right, up, forward) = self.camera_basis();
        let depth = dot(point, forward);
        let projected = egui::vec2(dot(point, right), dot(point, up));
        (projected, depth)
    }

    /// Project 3D point to 2D using pure orthographic projection (for fit calculations)
    pub fn project_3d_to_2d_ortho(&self, point: [f32; 3]) -> (egui::Vec2, f32) {
        let (right, up, forward) = self.camera_basis();
        let depth = dot(point, forward);
        let projected = egui::vec2(dot(point, right), dot(point, up));
        (projected, depth)
    }

    pub fn handle_input(&mut self, response: &egui::Response, ui: &egui::Ui) -> bool {
        if !response.contains_pointer() && !response.dragged() {
            return false;
        }

        let mut changed = false;
        let pointer_delta = ui.input(|input| input.pointer.delta());

        // Orbit rotation: left drag in 3D mode
        if self.view_mode == ViewMode::Orbit3D && response.dragged_by(egui::PointerButton::Primary)
        {
            self.yaw -= pointer_delta.x * 0.01;
            self.pitch = (self.pitch + pointer_delta.y * 0.01).clamp(-1.45, 1.45);
            changed = true;
        }

        // Pan: right drag or middle drag
        if response.dragged_by(egui::PointerButton::Secondary)
            || response.dragged_by(egui::PointerButton::Middle)
        {
            self.pan_offset += pointer_delta;
            changed = true;
        }

        // Zoom: scroll wheel zooms in/out
        if response.hovered() {
            let scroll_delta = ui.input(|input| input.raw_scroll_delta.y);

            // Normalize scroll: Windows gives ~120 per notch
            // Positive scroll = wheel up = zoom IN (make things bigger)
            // Negative scroll = wheel down = zoom OUT (make things smaller)
            let normalized = scroll_delta / 120.0;

            if normalized.abs() > 0.01 {
                // 1.15 per notch = 15% zoom change, feels responsive
                let zoom_factor = 1.15_f32.powf(normalized);
                let new_zoom = (self.zoom * zoom_factor).clamp(0.01, 100.0);
                self.zoom = new_zoom;
                changed = true;
            }
        }

        changed
    }

    pub fn set_view_mode(&mut self, view_mode: ViewMode) {
        self.view_mode = view_mode;
        self.pan_offset = egui::Vec2::ZERO;
        self.zoom = 1.0; // Reset zoom for proper zoom-to-fit

        match view_mode {
            ViewMode::XY => {
                self.yaw = 0.0;
                self.pitch = 0.0;
            }
            ViewMode::YZ => {
                self.yaw = std::f32::consts::FRAC_PI_2;
                self.pitch = 0.0;
            }
            ViewMode::ZX => {
                self.yaw = 0.0;
                self.pitch = 0.0;
            }
            ViewMode::Orbit3D => {
                // Initial view: origin at bottom-left, X-axis pointing upper-right
                // yaw = -π/4 rotates X-axis to point toward upper-right
                // pitch = 0.52 (~30°) gives a comfortable top-down angle
                self.yaw = -std::f32::consts::FRAC_PI_4;
                self.pitch = 0.52;
            }
        }
    }

    pub fn scale_factor(&self) -> f32 {
        self.zoom.max(0.1)
    }

    pub fn mode_label(view_mode: ViewMode) -> &'static str {
        match view_mode {
            ViewMode::XY => "X-Y",
            ViewMode::YZ => "Y-Z",
            ViewMode::ZX => "Z-X",
            ViewMode::Orbit3D => "Iso",
        }
    }

    pub fn camera_basis(&self) -> ([f32; 3], [f32; 3], [f32; 3]) {
        match self.view_mode {
            ViewMode::XY => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
            ViewMode::YZ => ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
            ViewMode::ZX => ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
            ViewMode::Orbit3D => {
                let yaw = self.yaw;
                let pitch = self.pitch.clamp(-1.45, 1.45);

                let forward = [
                    pitch.cos() * yaw.cos(),
                    pitch.cos() * yaw.sin(),
                    pitch.sin(),
                ];
                let right = [-yaw.sin(), yaw.cos(), 0.0];
                let up = [
                    -pitch.sin() * yaw.cos(),
                    -pitch.sin() * yaw.sin(),
                    pitch.cos(),
                ];

                (right, up, forward)
            }
        }
    }
}

impl Default for ViewState {
    fn default() -> Self {
        let mut state = Self {
            yaw: 0.0,
            pitch: 0.0,
            distance: 1.0,
            pan_offset: egui::Vec2::ZERO,
            zoom: 1.0,
            view_mode: ViewMode::Orbit3D,
        };
        state.set_view_mode(ViewMode::Orbit3D);
        state
    }
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_view_mode_switch() {
        let mut state = ViewState::default();
        state.set_view_mode(ViewMode::XY);
        assert_eq!(state.view_mode, ViewMode::XY);
        assert_eq!(state.pan_offset, egui::Vec2::ZERO);
    }

    #[test]
    fn test_xy_projection_uses_x_and_y() {
        let mut state = ViewState::default();
        state.set_view_mode(ViewMode::XY);
        let (point, _) = state.project_3d_to_2d_ortho([10.0, 20.0, 30.0]);
        assert_eq!(point, egui::vec2(10.0, 20.0));
    }

    #[test]
    fn test_yz_projection_uses_y_and_z() {
        let mut state = ViewState::default();
        state.set_view_mode(ViewMode::YZ);
        let (point, _) = state.project_3d_to_2d_ortho([10.0, 20.0, 30.0]);
        assert_eq!(point, egui::vec2(20.0, 30.0));
    }
}
