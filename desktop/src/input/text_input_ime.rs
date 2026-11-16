use crate::{
    PassApp,
    search::{ADVANCE, byte_at_utf16, utf16_at_byte},
};
use gpui::{Bounds, Context, EntityInputHandler, Pixels, Point, UTF16Selection, Window, px};
use std::ops::Range;

impl EntityInputHandler for PassApp {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        window: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        if self.state.custom_entry_name_focus.is_focused(window) {
            let start = byte_at_utf16(&self.state.custom_entry_name, range.start);
            let end = byte_at_utf16(&self.state.custom_entry_name, range.end).max(start);
            *adjusted = Some(
                utf16_at_byte(&self.state.custom_entry_name, start)
                    ..utf16_at_byte(&self.state.custom_entry_name, end),
            );
            return Some(self.state.custom_entry_name[start..end].to_owned());
        }
        if self.state.generator_name_focus.is_focused(window) {
            let start = byte_at_utf16(self.state.generator_name.as_str(), range.start);
            let end = byte_at_utf16(self.state.generator_name.as_str(), range.end).max(start);
            *adjusted = Some(
                utf16_at_byte(self.state.generator_name.as_str(), start)
                    ..utf16_at_byte(self.state.generator_name.as_str(), end),
            );
            return Some(self.state.generator_name[start..end].to_owned());
        }
        let start = byte_at_utf16(self.state.query.as_str(), range.start);
        let end = byte_at_utf16(self.state.query.as_str(), range.end).max(start);
        *adjusted = Some(
            utf16_at_byte(self.state.query.as_str(), start)
                ..utf16_at_byte(self.state.query.as_str(), end),
        );
        Some(self.state.query[start..end].to_owned())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        window: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        if self.state.custom_entry_name_focus.is_focused(window) {
            let selection = &self.state.custom_entry_name_selection;
            return Some(UTF16Selection {
                range: utf16_at_byte(&self.state.custom_entry_name, selection.start)
                    ..utf16_at_byte(&self.state.custom_entry_name, selection.end),
                reversed: false,
            });
        }
        let (text, selection) = if self.state.generator_name_focus.is_focused(window) {
            (
                self.state.generator_name.as_str(),
                &self.state.generator_name_selection,
            )
        } else {
            (self.state.query.as_str(), &self.state.selection)
        };
        Some(UTF16Selection {
            range: utf16_at_byte(text, selection.start)..utf16_at_byte(text, selection.end),
            reversed: self.state.generator_name_focus.is_focused(window)
                && self.state.generator_name_cursor < selection.start,
        })
    }

    fn marked_text_range(
        &self,
        window: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        if self.state.custom_entry_name_focus.is_focused(window) {
            return self.state.custom_entry_name_marked.as_ref().map(|range| {
                utf16_at_byte(&self.state.custom_entry_name, range.start)
                    ..utf16_at_byte(&self.state.custom_entry_name, range.end)
            });
        }
        let (text, marked) = if self.state.generator_name_focus.is_focused(window) {
            (
                self.state.generator_name.as_str(),
                &self.state.generator_name_marked,
            )
        } else {
            (self.state.query.as_str(), &self.state.marked)
        };
        marked
            .as_ref()
            .map(|range| utf16_at_byte(text, range.start)..utf16_at_byte(text, range.end))
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.state.custom_entry_name_focus.is_focused(window) {
            self.state.custom_entry_name_marked = None;
        } else if self.state.generator_name_focus.is_focused(window) {
            self.state.generator_name_marked = None;
        } else {
            self.state.marked = None;
        }
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.custom_entry_name_focus.is_focused(window) {
            self.replace_custom_entry_name(range, text, cx);
        } else if self.state.generator_name_focus.is_focused(window) {
            self.replace_generator_name(range, text, cx);
        } else {
            self.replace_search(range, text, cx);
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let custom_name_focused = self.state.custom_entry_name_focus.is_focused(window);
        if custom_name_focused {
            let inserted = self.replace_custom_entry_name(range, text, cx);
            self.state.custom_entry_name_marked =
                (!inserted.is_empty()).then_some(inserted.clone());
            if let Some(selected) = selected {
                let selection = inserted.start
                    + byte_at_utf16(
                        &self.state.custom_entry_name[inserted.clone()],
                        selected.start,
                    )
                    ..inserted.start
                        + byte_at_utf16(
                            &self.state.custom_entry_name[inserted.clone()],
                            selected.end,
                        );
                self.state.custom_entry_name_cursor = selection.end;
                self.state.custom_entry_name_selection = selection;
            }
            return;
        }
        let name_focused = self.state.generator_name_focus.is_focused(window);
        let inserted = if name_focused {
            self.replace_generator_name(range, text, cx)
        } else {
            self.replace_search(range, text, cx)
        };
        let (source, marked) = if name_focused {
            (
                self.state.generator_name.as_str(),
                &mut self.state.generator_name_marked,
            )
        } else {
            (self.state.query.as_str(), &mut self.state.marked)
        };
        *marked = (!inserted.is_empty()).then_some(inserted.clone());
        if let Some(selected) = selected {
            let text = &source[inserted.clone()];
            let selection = inserted.start + byte_at_utf16(text, selected.start)
                ..inserted.start + byte_at_utf16(text, selected.end);
            if name_focused {
                self.state.generator_name_cursor = selection.end;
                self.state.generator_name_selection = selection;
            } else {
                self.state.selection = selection;
            }
        }
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        if self.state.custom_entry_name_focus.is_focused(window) {
            let byte = byte_at_utf16(&self.state.custom_entry_name, range.start);
            let column = self.state.custom_entry_name[..byte].chars().count() as f32;
            return Some(Bounds::new(
                Point {
                    x: bounds.origin.x + px(column * ADVANCE),
                    y: bounds.origin.y,
                },
                gpui::size(px(1.), bounds.size.height),
            ));
        }
        let name_focused = self.state.generator_name_focus.is_focused(window);
        let text = if name_focused {
            self.state.generator_name.as_str()
        } else {
            self.state.query.as_str()
        };
        let byte = byte_at_utf16(text, range.start);
        if name_focused && self.text_editor_active() {
            let line = self
                .state
                .editor_layout
                .as_ref()?
                .lines
                .iter()
                .find(|line| byte >= line.range.start && byte <= line.range.end)
                .or_else(|| self.state.editor_layout.as_ref()?.lines.last())?;
            let offset = byte
                .saturating_sub(line.range.start)
                .min(line.range.end - line.range.start);
            return Some(Bounds::new(
                Point {
                    x: line.bounds.left() + line.shaped.x_for_index(offset),
                    y: line.bounds.top(),
                },
                gpui::size(px(1.), line.bounds.size.height),
            ));
        }
        let (column, row) =
            if self.state.generator_name_focus.is_focused(window) && self.text_editor_active() {
                let prefix = &text[..byte];
                let line_start = prefix.rfind('\n').map_or(0, |index| index + 1);
                (
                    text[line_start..byte].chars().count() as f32,
                    prefix.matches('\n').count() as f32,
                )
            } else {
                (text[..byte].chars().count() as f32, 0.)
            };
        Some(Bounds {
            origin: Point {
                x: bounds.origin.x + px(column * ADVANCE),
                y: bounds.origin.y + px(row * 26.),
            },
            size: gpui::size(
                px(1.),
                if self.state.generator_name_focus.is_focused(window) && self.text_editor_active() {
                    px(26.)
                } else {
                    bounds.size.height
                },
            ),
        })
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        if self.state.custom_entry_name_focus.is_focused(window) {
            let text = self.state.custom_entry_name.as_str();
            let column = ((point.x - self.state.custom_entry_name_bounds?.origin.x) / px(ADVANCE))
                .max(0.) as usize;
            return Some(text.chars().take(column).map(char::len_utf16).sum());
        }
        let name_focused = self.state.generator_name_focus.is_focused(window);
        let text = if name_focused {
            self.state.generator_name.as_str()
        } else {
            self.state.query.as_str()
        };
        if name_focused && self.text_editor_active() {
            let byte = self.state.editor_layout.as_ref()?.index_for_point(point);
            return Some(utf16_at_byte(text, byte));
        }
        let bounds = if name_focused {
            self.state.generator_name_bounds?
        } else {
            self.state.input_bounds?
        };
        let column = ((point.x - bounds.origin.x) / px(ADVANCE)).max(0.) as usize;
        Some(text.chars().take(column).map(char::len_utf16).sum())
    }
}
