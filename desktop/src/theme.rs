use gpui::{Div, FontWeight, Stateful, SharedString, div, prelude::*, px, rgb};

/// App-owned design tokens. Keep GPUI styling decisions here so a user theme
/// loader can replace these defaults without changing view construction.
#[derive(Clone, Debug)]
pub struct Theme {
    pub colors: Colors,
    pub spacing: Spacing,
    pub radii: Radii,
    pub mono_font: &'static str,
}

#[derive(Clone, Debug)]
pub struct Colors {
    pub background: u32,
    pub surface: u32,
    pub ink: u32,
    pub muted: u32,
    pub border: u32,
    pub accent: u32,
    pub accent_hover: u32,
    pub accent_surface: u32,
    pub danger: u32,
    pub danger_border: u32,
    pub danger_surface: u32,
    pub danger_surface_hover: u32,
    pub danger_hover: u32,
    pub hover: u32,
    pub placeholder: u32,
    pub nav_border: u32,
    pub nav_highlight: u32,
    pub nav_muted: u32,
    pub nav_selected: u32,
    pub nav_background: u32,
    pub nav_hover: u32,
    pub selection: u32,
}

#[derive(Clone, Debug)]
pub struct Spacing {
    pub control_height: f32,
    pub control_horizontal: f32,
    pub panel_padding: f32,
}

#[derive(Clone, Debug)]
pub struct Radii {
    pub control: f32,
    pub panel: f32,
    pub field: f32,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            colors: Colors {
                background: 0xf5f4ef,
                surface: 0xfbfaf7,
                ink: 0x232d29,
                muted: 0x667069,
                border: 0xd9dcd4,
                accent: 0x23694c,
                accent_hover: 0x19543c,
                accent_surface: 0xe4ece2,
                danger: 0x9d4035,
                danger_border: 0xe7c6c1,
                danger_surface: 0xf8e9e6,
                danger_surface_hover: 0xf1d9d5,
                danger_hover: 0x80352d,
                hover: 0xebeee6,
                placeholder: 0x8c968b,
                nav_border: 0x809287,
                nav_highlight: 0xe0edcb,
                nav_muted: 0xc7d6be,
                nav_selected: 0xd7ebbb,
                nav_background: 0x232d29,
                nav_hover: 0x39433e,
                selection: 0x334d8a68,
            },
            spacing: Spacing {
                control_height: 40.,
                control_horizontal: 18.,
                panel_padding: 22.,
            },
            radii: Radii { control: 0., panel: 0., field: 0. },
            mono_font: "DejaVu Sans Mono",
        }
    }
}

impl Theme {
    pub fn label(&self, text: impl Into<SharedString>) -> Div {
        div()
            .text_size(px(10.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(self.colors.muted))
            .child(text.into())
    }

    pub fn button(&self, id: &'static str, text: &'static str, primary: bool) -> Stateful<Div> {
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
            .border_color(rgb(if primary { colors.accent } else { colors.border }))
            .bg(rgb(if primary { colors.accent } else { colors.surface }))
            .text_color(rgb(if primary { colors.surface } else { colors.ink }))
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .cursor_pointer()
            .hover(move |style| style.bg(rgb(if primary { colors.accent_hover } else { colors.accent_surface })))
            .child(text)
    }

    pub fn danger_button(&self, id: &'static str, text: &'static str) -> Stateful<Div> {
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
            .child(text)
    }

    pub fn destructive_button(&self, id: &'static str, text: &'static str) -> Stateful<Div> {
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
            .child(text)
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
