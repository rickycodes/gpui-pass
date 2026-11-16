use gpui::{
    Animation, AnimationExt, Div, IntoElement, PathBuilder, canvas, div, point, prelude::*, px,
    rgba,
};
use lyon_tessellation::{LineCap, StrokeOptions};
use std::time::Duration;

const ROTATION_DURATION: Duration = Duration::from_millis(500);
const GRADIENT_LAYERS: usize = 24;
const TAIL_ANGLE: f32 = std::f32::consts::TAU * 0.82;
const RADIUS: f32 = 14.0;
const STROKE_WIDTH: f32 = 6.0;

pub(crate) fn loading_spinner(accent: u32) -> impl IntoElement {
    div().with_animation(
        "store-loading-spinner",
        Animation::new(ROTATION_DURATION).repeat(),
        move |_, progress| spinner_frame(accent, progress),
    )
}

fn spinner_frame(accent: u32, rotation_progress: f32) -> Div {
    div().size(px(40.)).child(
        canvas(
            |_, _, _| (),
            move |bounds, (), window, _| {
                let center_x = f32::from(bounds.origin.x) + f32::from(bounds.size.width) / 2.0;
                let center_y = f32::from(bounds.origin.y) + f32::from(bounds.size.height) / 2.0;
                let rotation = -std::f32::consts::TAU * rotation_progress;
                let radii = point(px(RADIUS), px(RADIUS));

                for layer in 0..GRADIENT_LAYERS {
                    let progress = layer as f32 / (GRADIENT_LAYERS - 1) as f32;
                    let arc_angle = TAIL_ANGLE * (1.0 - progress);
                    let start_angle = rotation + arc_angle;
                    let point_on_ring = |angle: f32| {
                        point(
                            px(center_x + RADIUS * angle.cos()),
                            px(center_y + RADIUS * angle.sin()),
                        )
                    };
                    let style = StrokeOptions::default()
                        .with_line_width(STROKE_WIDTH)
                        .with_line_cap(LineCap::Round);
                    let mut path = PathBuilder::stroke(px(STROKE_WIDTH))
                        .with_style(gpui::PathStyle::Stroke(style));
                    path.move_to(point_on_ring(start_angle));
                    path.arc_to(
                        radii,
                        px(0.),
                        arc_angle > std::f32::consts::PI,
                        false,
                        point_on_ring(rotation),
                    );
                    if let Ok(path) = path.build() {
                        let alpha = (0.035 + progress * 0.11) * 255.0;
                        let color = ((accent & 0x00ff_ffff) << 8) | alpha as u32;
                        window.paint_path(path, rgba(color));
                    }
                }
            },
        )
        .size_full(),
    )
}
