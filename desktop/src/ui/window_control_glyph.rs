use gpui::{IntoElement, PathBuilder, canvas, point, prelude::*, px, rgb};

pub(super) fn window_control_glyph(close: bool, color: u32) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            let center_x = f32::from(bounds.origin.x) + f32::from(bounds.size.width) / 2.0;
            let center_y = f32::from(bounds.origin.y) + f32::from(bounds.size.height) / 2.0;
            let half_size = 5.5;
            let mut path = PathBuilder::stroke(px(1.5));

            if close {
                path.move_to(point(px(center_x - half_size), px(center_y - half_size)));
                path.line_to(point(px(center_x + half_size), px(center_y + half_size)));
                path.move_to(point(px(center_x + half_size), px(center_y - half_size)));
                path.line_to(point(px(center_x - half_size), px(center_y + half_size)));
            } else {
                path.move_to(point(px(center_x - 6.0), px(center_y)));
                path.line_to(point(px(center_x + 6.0), px(center_y)));
            }

            if let Ok(path) = path.build() {
                window.paint_path(path, rgb(color));
            }
        },
    )
    .size_full()
}
