use eframe::egui::{self, Color32, Pos2, Rect, Shape, Stroke};

use super::view_state::ViewState;

/// Minimum 2D projected area (pixels²) for a face to be drawn.
/// Prevents degenerate near-zero polygons from causing egui tessellator artifacts.
const MIN_FACE_AREA: f32 = 0.5;

pub fn paint_axis_cube(painter: &egui::Painter, viewport: Rect, view_state: &ViewState) {
    let size = egui::vec2(50.0, 50.0);
    let margin = 12.0;
    let rect = Rect::from_min_size(
        egui::pos2(viewport.right() - size.x - margin, viewport.top() + margin),
        size,
    );

    // Isolate rendering to prevent artifacts bleeding into the main 3D scene.
    // clip_margin covers the axis labels drawn outside the box (up to ~14px tip + text).
    let clip_margin = 28.0;
    let clip_rect = rect.expand(clip_margin);
    let painter = painter.with_clip_rect(clip_rect);

    painter.rect_filled(rect, 6.0, Color32::from_black_alpha(120));
    painter.rect_stroke(rect, 6.0, Stroke::new(1.0, Color32::from_gray(90)));

    let center = rect.center();
    let half = 0.7_f32;

    // 8 vertices of the unit cube, indexed 0..7
    // Layout: 0-3 = bottom face (z=-half), 4-7 = top face (z=+half)
    //   3---2   7---6
    //   |   |   |   |
    //   0---1   4---5
    // x: 0=-, 1=+, 2=+, 3=-   (same for 4-7)
    // y: 0=-, 1=-, 2=+, 3=+   (same for 4-7)
    let verts: [[f32; 3]; 8] = [
        [-half, -half, -half], // 0
        [half, -half, -half],  // 1
        [half, half, -half],   // 2
        [-half, half, -half],  // 3
        [-half, -half, half],  // 4
        [half, -half, half],   // 5
        [half, half, half],    // 6
        [-half, half, half],   // 7
    ];

    // Project all 8 vertices once
    let projected: Vec<(egui::Vec2, f32)> = verts
        .iter()
        .map(|&v| view_state.project_3d_to_2d_ortho(v))
        .collect();

    // All 6 faces: (vertex indices CCW when viewed from outside, outward normal, color)
    // Winding is CCW as seen from *outside* the cube (outward normal direction).
    let faces: [([usize; 4], [f32; 3], Color32); 6] = [
        (
            [4, 5, 6, 7],
            [0.0, 0.0, 1.0],
            Color32::from_rgb(70, 120, 255),
        ), // +Z (top/blue)
        (
            [1, 0, 3, 2],
            [0.0, 0.0, -1.0],
            Color32::from_rgb(50, 80, 180),
        ), // -Z (bottom/dark blue)
        (
            [5, 1, 2, 6],
            [1.0, 0.0, 0.0],
            Color32::from_rgb(220, 80, 80),
        ), // +X (green)
        (
            [0, 4, 7, 3],
            [-1.0, 0.0, 0.0],
            Color32::from_rgb(160, 50, 50),
        ), // -X (dark red)
        (
            [3, 7, 6, 2],
            [0.0, 1.0, 0.0],
            Color32::from_rgb(80, 200, 120),
        ), // +Y (green)
        (
            [0, 1, 5, 4],
            [0.0, -1.0, 0.0],
            Color32::from_rgb(50, 140, 80),
        ), // -Y (dark green)
    ];

    // We use pure Painter's Algorithm: draw all 6 faces back-to-front by depth.
    // No back-face culling — culling direction depends on coordinate conventions
    // and is error-prone. Instead, sorting by depth (farthest first) naturally
    // hides back faces behind front faces, which is correct for a convex solid.

    // Build draw list: skip degenerate projections, compute depth for sorting
    let mut draw_list: Vec<(Vec<Pos2>, Color32, f32, usize)> = faces
        .iter()
        .enumerate()
        .filter_map(|(face_idx, (indices, _normal, color))| {
            // Build 2D screen points
            let points: Vec<Pos2> = indices
                .iter()
                .map(|&i| to_screen(center, projected[i].0, 14.0))
                .collect();

            // Skip degenerate (edge-on) polygons to avoid tessellator artifacts
            if polygon_area_2d(&points) < MIN_FACE_AREA {
                return None;
            }

            // Average depth along forward axis — larger = farther from camera
            let avg_depth =
                indices.iter().map(|&i| projected[i].1).sum::<f32>() / indices.len() as f32;

            Some((points, *color, avg_depth, face_idx))
        })
        .collect();

    // Sort back-to-front (painter's algorithm).
    // Tie-break by face_idx for a fully deterministic, stable order —
    // prevents depth-sort flipping when multiple faces share the same average depth
    // (which happens exactly when the view is orthogonal to one of the cube axes).
    draw_list.sort_by(|a, b| {
        b.2.partial_cmp(&a.2)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.3.cmp(&b.3))
    });

    for (points, color, _, _) in draw_list {
        painter.add(Shape::convex_polygon(
            points,
            color.gamma_multiply(0.75),
            Stroke::new(1.0, color),
        ));
    }

    // Axis lines (drawn on top of the faces)
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

/// Shoelace formula: signed area of a 2D polygon (returns absolute value).
fn polygon_area_2d(points: &[Pos2]) -> f32 {
    let n = points.len();
    if n < 3 {
        return 0.0;
    }
    let mut area = 0.0_f32;
    for i in 0..n {
        let j = (i + 1) % n;
        area += points[i].x * points[j].y;
        area -= points[j].x * points[i].y;
    }
    (area * 0.5).abs()
}
