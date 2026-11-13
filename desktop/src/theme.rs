use gpui::{Div, FontWeight, SharedString, Stateful, div, prelude::*, px, rgb};
use serde::Deserialize;
use std::{env, fs, path::PathBuf};

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

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ThemeOverrides {
    colors: ColorOverrides,
    spacing: SpacingOverrides,
    radii: RadiusOverrides,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ColorOverrides {
    background: Option<String>,
    surface: Option<String>,
    ink: Option<String>,
    muted: Option<String>,
    border: Option<String>,
    accent: Option<String>,
    accent_hover: Option<String>,
    accent_surface: Option<String>,
    danger: Option<String>,
    danger_border: Option<String>,
    danger_surface: Option<String>,
    danger_surface_hover: Option<String>,
    danger_hover: Option<String>,
    hover: Option<String>,
    placeholder: Option<String>,
    nav_border: Option<String>,
    nav_highlight: Option<String>,
    nav_muted: Option<String>,
    nav_selected: Option<String>,
    nav_background: Option<String>,
    nav_foreground: Option<String>,
    nav_hover: Option<String>,
    selection: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct SpacingOverrides {
    control_height: Option<f32>,
    control_horizontal: Option<f32>,
    panel_padding: Option<f32>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RadiusOverrides {
    control: Option<f32>,
    panel: Option<f32>,
    field: Option<f32>,
}

impl Theme {
    /// Load the optional user theme over the built-in defaults. Invalid files
    /// are ignored as a whole so partial parsing cannot leave a mixed theme.
    pub fn load_user() -> Self {
        let defaults = Self::default();
        let Some(path) = theme_config_path() else {
            return defaults;
        };
        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return defaults,
            Err(error) => {
                eprintln!("Could not read theme config {}: {error}", path.display());
                return defaults;
            }
        };
        let result = toml::from_str::<ThemeOverrides>(&source)
            .map_err(|error| error.to_string())
            .and_then(|overrides| overrides.apply(defaults.clone()));
        match result {
            Ok(theme) => theme,
            Err(error) => {
                eprintln!("Ignoring invalid theme config {}: {error}", path.display());
                defaults
            }
        }
    }

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
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .cursor_pointer()
            .hover(move |style| {
                style.bg(rgb(if primary {
                    colors.accent_hover
                } else {
                    colors.accent_surface
                }))
            })
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

impl ThemeOverrides {
    fn apply(self, mut theme: Theme) -> Result<Theme, String> {
        macro_rules! color {
            ($field:ident) => {
                if let Some(value) = self.colors.$field {
                    theme.colors.$field = parse_color(stringify!($field), &value)?;
                }
            };
        }
        color!(background);
        color!(surface);
        color!(ink);
        color!(muted);
        color!(border);
        color!(accent);
        color!(accent_hover);
        color!(accent_surface);
        color!(danger);
        color!(danger_border);
        color!(danger_surface);
        color!(danger_surface_hover);
        color!(danger_hover);
        color!(hover);
        color!(placeholder);
        color!(nav_border);
        color!(nav_highlight);
        color!(nav_muted);
        color!(nav_selected);
        color!(nav_background);
        color!(nav_foreground);
        color!(nav_hover);
        color!(selection);

        metric(
            &mut theme.spacing.control_height,
            self.spacing.control_height,
            "spacing.control_height",
            24.0,
            80.0,
        )?;
        metric(
            &mut theme.spacing.control_horizontal,
            self.spacing.control_horizontal,
            "spacing.control_horizontal",
            0.0,
            48.0,
        )?;
        metric(
            &mut theme.spacing.panel_padding,
            self.spacing.panel_padding,
            "spacing.panel_padding",
            0.0,
            64.0,
        )?;
        metric(
            &mut theme.radii.control,
            self.radii.control,
            "radii.control",
            0.0,
            32.0,
        )?;
        metric(
            &mut theme.radii.panel,
            self.radii.panel,
            "radii.panel",
            0.0,
            32.0,
        )?;
        metric(
            &mut theme.radii.field,
            self.radii.field,
            "radii.field",
            0.0,
            32.0,
        )?;
        Ok(theme)
    }
}

fn parse_color(name: &str, value: &str) -> Result<u32, String> {
    let digits = value.strip_prefix('#').unwrap_or(value);
    let expected_len = if name == "selection" { 8 } else { 6 };
    if digits.len() != expected_len || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        let format = if expected_len == 8 {
            "#RRGGBBAA"
        } else {
            "#RRGGBB"
        };
        return Err(format!("colors.{name} must be {format}"));
    }
    u32::from_str_radix(digits, 16)
        .map_err(|_| format!("colors.{name} is not a valid hexadecimal color"))
}

fn metric(
    target: &mut f32,
    value: Option<f32>,
    name: &str,
    min: f32,
    max: f32,
) -> Result<(), String> {
    if let Some(value) = value {
        if !value.is_finite() || !(min..=max).contains(&value) {
            return Err(format!("{name} must be between {min} and {max}"));
        }
        *target = value;
    }
    Ok(())
}

fn theme_config_path() -> Option<PathBuf> {
    env::var_os("XDG_CONFIG_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .map(|directory| directory.join("gpui-pass").join("theme.toml"))
}
