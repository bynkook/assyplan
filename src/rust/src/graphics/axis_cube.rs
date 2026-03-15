use eframe::egui::{self, Color32, Id, LayerId, Order, Pos2, Rect, Shape, Stroke};

use super::view_state::ViewState;

/// Minimum 2D projected area (pixels²) for a face to be drawn.
/// Prevents degenerate near-zero polygons from causing egui tessellator artifacts.
const MIN_FACE_AREA: f32 = 0.5;

pub fn paint_axis_cube(ctx: &egui::Context, viewport: Rect, view_state: &ViewState) {
    let size = egui::vec2(50.0, 50.0);
    let margin = 12.0;
    let rect = Rect::from_min_size(
        egui::pos2(viewport.right() - size.x - margin, viewport.top() + margin),
        size,
    );

    // Use a completely independent Foreground layer so the painter's clip_rect
    // starts from content_rect (full screen), not from the viewport painter's clip_rect.
    // This makes with_clip_rect(rect) actually work as a hard boundary.
    //
    // Background: ui.painter_at(viewport) creates a painter clipped to viewport.
    // Calling painter.with_clip_rect(small_rect) does small_rect.intersect(viewport),
    // which (when small_rect ⊂ viewport) equals small_rect — but only if small_rect is
    // strictly inside the viewport. When expand() is used it may overflow and fail.
    // A new layer_painter has clip_rect = content_rect (full screen), so
    // with_clip_rect(rect) reliably clips to exactly rect. No bleeding.
    let layer_id = LayerId::new(Order::Foreground, Id::new("axis_cube_overlay"));
    let base_painter = ctx.layer_painter(layer_id);

    // Clip the foreground painter to just the cube rect (no expand needed —
    // faces and axis lines are all within this rect by construction).
    let painter = base_painter.with_clip_rect(rect);

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
        ), // +X (red)
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

    // Rendering strategy:
    // 1. Back-face culling: skip faces whose outward normal points away from camera.
    //    dot(normal, forward) < 0  →  back-facing  →  skip.
    //    This alone prevents back faces from ever being drawn, so painter's algorithm
    //    only needs to order the (at most 3) visible front faces correctly.
    // 2. Painter's algorithm on the remaining front faces: draw farthest first.
    //    "Farthest" = smallest depth value along forward axis
    //    (depth = dot(vertex, forward); smaller = farther from camera in ortho).

    let (_, _, forward) = view_state.camera_basis();

    // Build draw list: cull back-faces, skip degenerate projections, compute depth
    let mut draw_list: Vec<(Vec<Pos2>, Color32, f32, usize)> = faces
        .iter()
        .enumerate()
        .filter_map(|(face_idx, (indices, normal, color))| {
            // Back-face culling: dot(outward normal, forward) must be > 0
            // to be front-facing (normal pointing toward camera).
            let n_dot_f = dot(*normal, forward);
            if n_dot_f <= 0.0 {
                return None;
            }

            // Build 2D screen points
            let points: Vec<Pos2> = indices
                .iter()
                .map(|&i| to_screen(center, projected[i].0, 14.0))
                .collect();

            // Skip degenerate (edge-on) polygons to avoid tessellator artifacts.
            // In practice, back-face culling already removes most edge-on faces;
            // this is a safety net for the exactly-orthogonal case.
            if polygon_area_2d(&points) < MIN_FACE_AREA {
                return None;
            }

            // Average depth: dot(vertex, forward). Smaller = farther from camera.
            let avg_depth =
                indices.iter().map(|&i| projected[i].1).sum::<f32>() / indices.len() as f32;

            Some((points, *color, avg_depth, face_idx))
        })
        .collect();

    // Sort back-to-front: draw farthest faces first (smallest depth first).
    // Tie-break by face_idx for fully deterministic, stable order.
    draw_list.sort_by(|a, b| {
        a.2.partial_cmp(&b.2)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.3.cmp(&b.3))
    });

    for (points, color, _, _) in draw_list {
        // Fill only — no stroke. Stroke on convex_polygon bleeds outside clip_rect
        // in some egui backends. Face border effect achieved by slightly darker fill.
        painter.add(Shape::convex_polygon(
            points,
            color.gamma_multiply(0.75),
            Stroke::NONE,
        ));
    }

    // Axis lines (drawn on top of the faces)
    // scale=16.0 and direction vectors have magnitude ≤ 1.1, so tip is at most
    // center ± 17.6px — well within the 50x50 rect (25px radius from center).
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

/// Dot product of two 3D vectors.
#[inline]
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
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
