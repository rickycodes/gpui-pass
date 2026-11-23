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
    pub nav_foreground: u32,
    pub nav_hover: u32,
    pub selection: u32,
}

#[derive(Clone, Debug)]
pub struct Spacing {
    pub control_height: f32,
    pub control_horizontal: f32,
    pub panel_padding: f32,
    pub field_padding: f32,
    pub chrome_height: f32,
    pub content_horizontal: f32,
    pub content_vertical: f32,
    pub composer_horizontal: f32,
    pub composer_vertical: f32,
    pub footer_horizontal: f32,
    pub section_gap: f32,
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
                nav_foreground: 0xfbfaf7,
                nav_hover: 0x39433e,
                selection: 0x334d8a68,
            },
            spacing: Spacing {
                control_height: 40.,
                control_horizontal: 18.,
                panel_padding: 22.,
                field_padding: 12.,
                chrome_height: 48.,
                content_horizontal: 40.,
                content_vertical: 32.,
                composer_horizontal: 32.,
                composer_vertical: 48.,
                footer_horizontal: 24.,
                section_gap: 16.,
            },
            radii: Radii {
                control: 0.,
                panel: 0.,
                field: 0.,
            },
            mono_font: "DejaVu Sans Mono",
        }
    }
}
