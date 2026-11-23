use super::*;

impl Theme {
    pub fn page_title(&self, text: impl Into<SharedString>) -> Div {
        div()
            .text_size(px(32.))
            .line_height(px(40.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(rgb(self.colors.ink))
            .child(text.into())
    }

    pub fn panel_title(&self, text: impl Into<SharedString>) -> Div {
        self.page_title(text)
            .text_size(px(24.))
            .line_height(px(32.))
    }

    pub fn status_badge(&self, text: impl Into<SharedString>) -> Div {
        div()
            .flex()
            .flex_shrink_0()
            .items_center()
            .gap(px(6.))
            .h(px(26.))
            .px(px(10.))
            .rounded(px(self.radii.control))
            .bg(rgb(self.colors.accent_surface))
            .text_size(px(11.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(rgb(self.colors.accent))
            .child(
                div()
                    .size(px(5.))
                    .rounded_full()
                    .bg(rgb(self.colors.accent)),
            )
            .child(text.into())
    }

    pub fn nav_link(
        &self,
        id: &'static str,
        text: impl Into<SharedString>,
        active: bool,
    ) -> Stateful<Div> {
        let colors = self.colors.clone();
        div()
            .id(id)
            .h(px(30.))
            .px(px(10.))
            .flex()
            .items_center()
            .rounded(px(self.radii.control))
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(rgb(if active {
                colors.nav_selected
            } else {
                colors.nav_foreground
            }))
            .bg(rgb(if active {
                colors.nav_hover
            } else {
                colors.nav_background
            }))
            .cursor_pointer()
            .hover(move |style| style.bg(rgb(colors.nav_hover)))
            .child(text.into())
    }

    pub fn shortcut(&self, keys: &'static str, label: impl Into<SharedString>) -> Div {
        div()
            .flex()
            .flex_shrink_0()
            .items_center()
            .gap(px(7.))
            .text_size(px(11.))
            .text_color(rgb(self.colors.muted))
            .child(
                div()
                    .px(px(5.))
                    .py(px(2.))
                    .rounded(px(self.radii.control.min(4.)))
                    .border_1()
                    .border_color(rgb(self.colors.border))
                    .font_family(self.mono_font)
                    .text_size(px(10.))
                    .child(keys),
            )
            .child(label.into())
    }

    pub fn label(&self, text: impl Into<SharedString>) -> Div {
        div()
            .text_size(px(11.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(self.colors.muted))
            .child(text.into())
    }

    pub fn button(
        &self,
        id: &'static str,
        text: impl Into<SharedString>,
        primary: bool,
    ) -> Stateful<Div> {
        let colors = self.colors.clone();
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_center()
            .h(px(self.spacing.control_height))
            .px(px(self.spacing.control_horizontal))
            .rounded(px(self.radii.control))
            .border_1()
            .border_color(rgb(if primary {
                colors.accent
            } else {
                colors.border
            }))
            .bg(rgb(if primary {
                colors.accent
            } else {
                colors.surface
            }))
            .text_color(rgb(if primary { colors.surface } else { colors.ink }))
            .text_size(px(13.))
            .font_weight(FontWeight::MEDIUM)
            .cursor_pointer()
            .hover(move |style| {
                style.bg(rgb(if primary {
                    colors.accent_hover
                } else {
                    colors.accent_surface
                }))
            })
            .child(text.into())
    }

    pub fn danger_button(&self, id: &'static str, text: impl Into<SharedString>) -> Stateful<Div> {
        let colors = self.colors.clone();
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_center()
            .h(px(self.spacing.control_height))
            .px(px(self.spacing.control_horizontal))
            .rounded(px(self.radii.control))
            .border_1()
            .border_color(rgb(colors.danger_border))
            .bg(rgb(colors.danger_surface))
            .text_color(rgb(colors.danger))
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .cursor_pointer()
            .hover(move |style| style.bg(rgb(colors.danger_surface_hover)))
            .child(text.into())
    }

    pub fn destructive_button(
        &self,
        id: &'static str,
        text: impl Into<SharedString>,
    ) -> Stateful<Div> {
        let colors = self.colors.clone();
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_center()
            .h(px(self.spacing.control_height))
            .px(px(self.spacing.control_horizontal))
            .rounded(px(self.radii.control))
            .border_1()
            .border_color(rgb(colors.danger))
            .bg(rgb(colors.danger))
            .text_color(rgb(colors.surface))
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .cursor_pointer()
            .hover(move |style| style.bg(rgb(colors.danger_hover)))
            .child(text.into())
    }

    pub fn panel(&self) -> Div {
        div()
            .rounded(px(self.radii.panel))
            .p(px(self.spacing.panel_padding))
            .border_1()
            .border_color(rgb(self.colors.border))
            .bg(rgb(self.colors.surface))
    }
}
