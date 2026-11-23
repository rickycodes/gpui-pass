use super::*;

const RESIZE_EDGE_SIZE: f32 = 8.;
const RESIZE_CORNER_SIZE: f32 = 14.;

impl Render for PassApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render_sample = diagnostics::measure_render();
        self.advance_sidebar_scroll(window);
        let onboarding = self.state.onboarding.is_some();
        let main_content = if onboarding {
            self.onboarding_view(window, cx)
        } else if self.state.navigation.composer_open() {
            self.composer_view(window, cx)
        } else if self.state.navigation.page() == Page::CardDetails {
            self.yubikey_view(cx)
        } else {
            div()
                .flex()
                .flex_1()
                .min_h_0()
                .child(self.sidebar(window, cx))
                .child(self.details(window, cx))
                .into_any_element()
        };
        let footer = self.footer(cx);
        app_actions::bind_actions(
            div()
                .size_full()
                .flex()
                .flex_col()
                .bg(rgb(self.state.theme.colors.background))
                .text_color(rgb(self.state.theme.colors.ink))
                .font_family("Inter")
                .text_size(px(15.))
                .key_context("Pass")
                .track_focus(&self.state.focus),
            cx,
        )
        .when(
            !self.state.navigation.composer_open() && !onboarding,
            |view| view.child(self.titlebar("titlebar", Some(self.navigation_links(cx)), cx)),
        )
        .child(main_content)
        .child(footer)
        .when(self.state.confirming_delete.is_some(), |view| {
            view.child(self.delete_confirmation_view(cx))
        })
        .when(self.state.confirming_overwrite.is_some(), |view| {
            view.child(self.overwrite_confirmation_view(cx))
        })
        .child(resize_handle(
            "resize-top",
            ResizeEdge::Top,
            CursorStyle::ResizeUpDown,
            ResizeHandleBounds {
                top: Some(0.),
                left: Some(RESIZE_EDGE_SIZE),
                right: Some(RESIZE_EDGE_SIZE),
                height: Some(RESIZE_EDGE_SIZE),
                ..Default::default()
            },
        ))
        .child(resize_handle(
            "resize-bottom",
            ResizeEdge::Bottom,
            CursorStyle::ResizeUpDown,
            ResizeHandleBounds {
                left: Some(RESIZE_EDGE_SIZE),
                right: Some(RESIZE_EDGE_SIZE),
                bottom: Some(0.),
                height: Some(RESIZE_EDGE_SIZE),
                ..Default::default()
            },
        ))
        .child(resize_handle(
            "resize-left",
            ResizeEdge::Left,
            CursorStyle::ResizeLeftRight,
            ResizeHandleBounds {
                top: Some(RESIZE_EDGE_SIZE),
                left: Some(0.),
                bottom: Some(RESIZE_EDGE_SIZE),
                width: Some(RESIZE_EDGE_SIZE),
                ..Default::default()
            },
        ))
        .child(resize_handle(
            "resize-right",
            ResizeEdge::Right,
            CursorStyle::ResizeLeftRight,
            ResizeHandleBounds {
                top: Some(RESIZE_EDGE_SIZE),
                right: Some(0.),
                bottom: Some(RESIZE_EDGE_SIZE),
                width: Some(RESIZE_EDGE_SIZE),
                ..Default::default()
            },
        ))
        .child(resize_handle(
            "resize-top-left",
            ResizeEdge::TopLeft,
            CursorStyle::ResizeUpLeftDownRight,
            ResizeHandleBounds {
                top: Some(0.),
                left: Some(0.),
                width: Some(RESIZE_CORNER_SIZE),
                height: Some(RESIZE_CORNER_SIZE),
                ..Default::default()
            },
        ))
        .child(resize_handle(
            "resize-top-right",
            ResizeEdge::TopRight,
            CursorStyle::ResizeUpRightDownLeft,
            ResizeHandleBounds {
                top: Some(0.),
                right: Some(0.),
                width: Some(RESIZE_CORNER_SIZE),
                height: Some(RESIZE_CORNER_SIZE),
                ..Default::default()
            },
        ))
        .child(resize_handle(
            "resize-bottom-left",
            ResizeEdge::BottomLeft,
            CursorStyle::ResizeUpRightDownLeft,
            ResizeHandleBounds {
                left: Some(0.),
                bottom: Some(0.),
                width: Some(RESIZE_CORNER_SIZE),
                height: Some(RESIZE_CORNER_SIZE),
                ..Default::default()
            },
        ))
        .child(resize_handle(
            "resize-bottom-right",
            ResizeEdge::BottomRight,
            CursorStyle::ResizeUpLeftDownRight,
            ResizeHandleBounds {
                right: Some(0.),
                bottom: Some(0.),
                width: Some(RESIZE_CORNER_SIZE),
                height: Some(RESIZE_CORNER_SIZE),
                ..Default::default()
            },
        ))
    }
}

#[derive(Default)]
struct ResizeHandleBounds {
    top: Option<f32>,
    left: Option<f32>,
    right: Option<f32>,
    bottom: Option<f32>,
    width: Option<f32>,
    height: Option<f32>,
}

fn resize_handle(
    id: &'static str,
    edge: ResizeEdge,
    cursor: CursorStyle,
    bounds: ResizeHandleBounds,
) -> gpui::Stateful<gpui::Div> {
    let mut handle = div().id(id).absolute().cursor(cursor).on_mouse_down(
        gpui::MouseButton::Left,
        move |_, window, cx| {
            window.start_window_resize(edge);
            cx.stop_propagation();
        },
    );
    if let Some(top) = bounds.top {
        handle = handle.top(px(top));
    }
    if let Some(left) = bounds.left {
        handle = handle.left(px(left));
    }
    if let Some(right) = bounds.right {
        handle = handle.right(px(right));
    }
    if let Some(bottom) = bounds.bottom {
        handle = handle.bottom(px(bottom));
    }
    if let Some(width) = bounds.width {
        handle = handle.w(px(width));
    }
    if let Some(height) = bounds.height {
        handle = handle.h(px(height));
    }
    handle
}
