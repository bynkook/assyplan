use eframe::egui::{self, Color32, Pos2, Rect, Shape, Stroke};

use super::view_state::ViewState;

pub fn paint_axis_cube(painter: &egui::Painter, viewport: Rect, view_state: &ViewState) {
    let size = egui::vec2(50.0, 50.0);
    let margin = 12.0;
    let rect = Rect::from_min_size(
        egui::pos2(viewport.right() - size.x - margin, viewport.top() + margin),
        size,
    );

    // Use a clip rect slightly larger than the cube rect to isolate rendering
    // and prevent artifacts bleeding into the main 3D scene painter
    let clip_margin = 20.0; // enough to include axis labels drawn outside the box
    let clip_rect = rect.expand(clip_margin);
    let painter = painter.with_clip_rect(clip_rect);

    painter.rect_filled(rect, 6.0, Color32::from_black_alpha(120));
    painter.rect_stroke(rect, 6.0, Stroke::new(1.0, Color32::from_gray(90)));

    let center = rect.center();
    let half = 0.7;
    let vertices = [
        [-half, -half, -half],
        [half, -half, -half],
        [half, half, -half],
        [-half, half, -half],
        [-half, -half, half],
        [half, -half, half],
        [half, half, half],
        [-half, half, half],
    ];

    let projected: Vec<(egui::Vec2, f32)> = vertices
        .iter()
        .map(|&vertex| view_state.project_3d_to_2d_ortho(vertex))
        .collect();

    let faces = [
        ([4, 5, 6, 7], Color32::from_rgb(70, 120, 255)),
        ([1, 2, 6, 5], Color32::from_rgb(80, 200, 120)),
        ([3, 2, 6, 7], Color32::from_rgb(220, 80, 80)),
    ];

    let mut sorted_faces: Vec<_> = faces
        .iter()
        .map(|(indices, color)| {
            let depth = indices.iter().map(|i| projected[*i].1).sum::<f32>() / indices.len() as f32;
            (*indices, *color, depth)
        })
        .collect();
    sorted_faces.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

    for (indices, color, _) in sorted_faces {
        let points: Vec<Pos2> = indices
            .iter()
            .map(|i| to_screen(center, projected[*i].0, 14.0))
            .collect();
        painter.add(Shape::convex_polygon(
            points,
            color.gamma_multiply(0.75),
            Stroke::new(1.0, color),
        ));
    }

    paint_axis_line(
        &painter,
        center,
        view_state.project_3d_to_2d_ortho([1.1, 0.0, 0.0]).0,
        16.0,
        Color32::from_rgb(220, 80, 80),
        "X",
    );
    paint_axis_line(
        &painter,
        center,
        view_state.project_3d_to_2d_ortho([0.0, 1.1, 0.0]).0,
        16.0,
        Color32::from_rgb(80, 200, 120),
        "Y",
    );
    paint_axis_line(
        &painter,
        center,
        view_state.project_3d_to_2d_ortho([0.0, 0.0, 1.1]).0,
        16.0,
        Color32::from_rgb(70, 120, 255),
        "Z",
    );
}

fn to_screen(center: Pos2, projected: egui::Vec2, scale: f32) -> Pos2 {
    center + egui::vec2(projected.x * scale, -projected.y * scale)
}

fn paint_axis_line(
    painter: &egui::Painter,
    center: Pos2,
    direction: egui::Vec2,
    scale: f32,
    color: Color32,
    label: &str,
) {
    let tip = to_screen(center, direction, scale);
    painter.line_segment([center, tip], Stroke::new(2.0, color));
    painter.circle_filled(tip, 2.5, color);
    painter.text(
        tip + egui::vec2(4.0, -2.0),
        egui::Align2::LEFT_BOTTOM,
        label,
        egui::FontId::proportional(10.0),
        Color32::WHITE,
    );
}
