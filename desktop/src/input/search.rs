use crate::PassApp;
use gpui::{Context, Hsla, Pixels, Point, ShapedLine, TextRun, Window, font, px};
use std::ops::Range;

fn input_text_layout(window: &mut Window, text: &str, font_family: &'static str) -> ShapedLine {
    window.text_system().shape_line(
        text.to_owned().into(),
        px(13.),
        &[TextRun {
            len: text.len(),
            font: font(font_family),
            color: Hsla::default(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }],
        None,
    )
}

pub(super) fn input_byte_at_x(
    window: &mut Window,
    text: &str,
    x: Pixels,
    origin_x: Pixels,
    font_family: &'static str,
) -> usize {
    let layout = input_text_layout(window, text, font_family);
    layout.closest_index_for_x((x - origin_x).max(px(0.)))
}

pub(super) fn input_x_for_byte(
    window: &mut Window,
    text: &str,
    byte: usize,
    origin_x: Pixels,
    font_family: &'static str,
) -> Pixels {
    let layout = input_text_layout(window, text, font_family);
    origin_x + layout.x_for_index(byte.min(text.len()))
}

pub(super) fn byte_at_utf16(text: &str, offset: usize) -> usize {
    let mut utf16 = 0;
    for (byte, character) in text.char_indices() {
        if utf16 + character.len_utf16() > offset {
            return byte;
        }
        utf16 += character.len_utf16();
    }
    text.len()
}

pub(super) fn utf16_at_byte(text: &str, offset: usize) -> usize {
    text[..offset].encode_utf16().count()
}

impl PassApp {
    pub(super) fn replace_custom_entry_name(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        cx: &mut Context<Self>,
    ) -> Range<usize> {
        let range = range
            .map(|range| {
                byte_at_utf16(&self.state.custom_entry_name, range.start)
                    ..byte_at_utf16(&self.state.custom_entry_name, range.end)
            })
            .or_else(|| self.state.custom_entry_name_marked.clone())
            .unwrap_or_else(|| self.state.custom_entry_name_selection.clone());
        let start = range.start.min(range.end);
        let end = range.end.max(range.start);
        let room = 512_usize.saturating_sub(
            self.state.custom_entry_name[..start].chars().count()
                + self.state.custom_entry_name[end..].chars().count(),
        );
        let text: String = text
            .chars()
            .filter(|c| !c.is_control())
            .take(room)
            .collect();
        self.state
            .custom_entry_name
            .replace_range(start..end, &text);
        let inserted = start..start + text.len();
        self.state.custom_entry_name_selection = inserted.end..inserted.end;
        self.state.custom_entry_name_cursor = inserted.end;
        self.state.custom_entry_name_anchor = None;
        self.state.custom_entry_name_marked = None;
        self.clear_message();
        cx.notify();
        inserted
    }

    pub(crate) fn select_custom_entry_name(&mut self, delta: isize, cx: &mut Context<Self>) {
        let cursor = self.state.custom_entry_name_cursor;
        let anchor = *self.state.custom_entry_name_anchor.get_or_insert(cursor);
        let next = if delta < 0 {
            self.state.custom_entry_name[..cursor]
                .char_indices()
                .next_back()
                .map_or(0, |(i, _)| i)
        } else {
            cursor
                + self.state.custom_entry_name[cursor..]
                    .chars()
                    .next()
                    .map_or(0, char::len_utf8)
        };
        self.state.custom_entry_name_selection = anchor.min(next)..anchor.max(next);
        self.state.custom_entry_name_cursor = next;
        cx.notify();
    }

    pub(super) fn replace_generator_name(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        cx: &mut Context<Self>,
    ) -> Range<usize> {
        if self.state.navigation.generator_open() {
            self.state.content_generated = false;
        }
        let range = range
            .map(|range| {
                byte_at_utf16(self.state.generator_name.as_str(), range.start)
                    ..byte_at_utf16(self.state.generator_name.as_str(), range.end)
            })
            .or_else(|| self.state.generator_name_marked.clone())
            .unwrap_or_else(|| self.state.generator_name_selection.clone());
        let start = range.start.min(range.end);
        let end = range.end.max(range.start);
        let text = if self.text_editor_active() {
            zeroize::Zeroizing::new(text.to_owned())
        } else {
            let room = 512_usize.saturating_sub(
                self.state.generator_name[..start].chars().count()
                    + self.state.generator_name[end..].chars().count(),
            );
            zeroize::Zeroizing::new(
                text.chars()
                    .filter(|c| !c.is_control())
                    .take(room)
                    .collect::<String>(),
            )
        };
        if self.text_editor_active() {
            let Some(next) =
                crate::editor_input::replace(&self.state.generator_name, start..end, &text)
            else {
                self.set_message(crate::i18n::text("editor-limit"));
                cx.notify();
                return start..start;
            };
            self.state.generator_name = next;
            self.state.editor_layout = None;
        } else {
            self.state.generator_name.replace_range(start..end, &text);
        }
        let inserted = start..start + text.len();
        self.state.generator_name_selection = inserted.end..inserted.end;
        self.state.generator_name_cursor = inserted.end;
        self.state.generator_name_anchor = None;
        self.state.generator_name_marked = None;
        self.clear_message();
        cx.notify();
        inserted
    }

    pub(super) fn replace_search(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        cx: &mut Context<Self>,
    ) -> Range<usize> {
        let range = range
            .map(|range| {
                byte_at_utf16(self.state.query.as_str(), range.start)
                    ..byte_at_utf16(self.state.query.as_str(), range.end)
            })
            .or_else(|| self.state.marked.clone())
            .unwrap_or_else(|| self.state.selection.clone());
        let start = range.start.min(range.end);
        let end = range.end.max(range.start);
        let room = 512_usize.saturating_sub(
            self.state.query[..start].chars().count() + self.state.query[end..].chars().count(),
        );
        let text: String = text
            .chars()
            .filter(|c| !c.is_control())
            .take(room)
            .collect();
        self.state.query.replace_range(start..end, &text);
        let inserted = start..start + text.len();
        self.state.selection = inserted.end..inserted.end;
        self.state.marked = None;
        self.filter(cx);
        cx.notify();
        inserted
    }

    pub(crate) fn backspace(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.state.custom_entry_name_focus.is_focused(window) {
            if self.state.custom_entry_name_selection.is_empty() {
                self.state.custom_entry_name_selection.start = self.state.custom_entry_name
                    [..self.state.custom_entry_name_selection.start]
                    .char_indices()
                    .next_back()
                    .map_or(0, |(byte, _)| byte);
            }
            self.replace_custom_entry_name(None, "", cx);
            return;
        }
        if self.state.generator_name_focus.is_focused(window) {
            if self.state.generator_name_selection.is_empty() {
                self.state.generator_name_selection.start = self.state.generator_name
                    [..self.state.generator_name_selection.start]
                    .char_indices()
                    .next_back()
                    .map_or(0, |(byte, _)| byte);
            }
            self.replace_generator_name(None, "", cx);
            return;
        }
        if !self.state.search_focus.is_focused(window) {
            return;
        }
        if self.state.selection.is_empty() {
            let previous = self.state.query[..self.state.selection.start]
                .char_indices()
                .next_back()
                .map_or(0, |(byte, _)| byte);
            self.state.selection.start = previous;
        }
        self.replace_search(None, "", cx);
    }

    pub(crate) fn delete(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.state.custom_entry_name_focus.is_focused(window) {
            if self.state.custom_entry_name_selection.is_empty() {
                self.state.custom_entry_name_selection.end += self.state.custom_entry_name
                    [self.state.custom_entry_name_selection.end..]
                    .chars()
                    .next()
                    .map_or(0, char::len_utf8);
            }
            self.replace_custom_entry_name(None, "", cx);
            return;
        }
        if self.state.generator_name_focus.is_focused(window) {
            if self.state.generator_name_selection.is_empty() {
                self.state.generator_name_selection.end += self.state.generator_name
                    [self.state.generator_name_selection.end..]
                    .chars()
                    .next()
                    .map_or(0, char::len_utf8);
            }
            self.replace_generator_name(None, "", cx);
            return;
        }
        if !self.state.search_focus.is_focused(window) {
            return;
        }
        if self.state.selection.is_empty() {
            self.state.selection.end += self.state.query[self.state.selection.end..]
                .chars()
                .next()
                .map_or(0, char::len_utf8);
        }
        self.replace_search(None, "", cx);
    }

    pub(crate) fn move_cursor(&mut self, delta: isize, window: &Window, cx: &mut Context<Self>) {
        if self.state.custom_entry_name_focus.is_focused(window) {
            let selection = &self.state.custom_entry_name_selection;
            let byte = if !selection.is_empty() {
                if delta < 0 {
                    selection.start
                } else {
                    selection.end
                }
            } else if delta < 0 {
                self.state.custom_entry_name[..selection.start]
                    .char_indices()
                    .next_back()
                    .map_or(0, |(i, _)| i)
            } else {
                selection.end
                    + self.state.custom_entry_name[selection.end..]
                        .chars()
                        .next()
                        .map_or(0, char::len_utf8)
            };
            self.state.custom_entry_name_selection = byte..byte;
            self.state.custom_entry_name_cursor = byte;
            self.state.custom_entry_name_anchor = None;
            cx.notify();
            return;
        }
        if self.state.generator_name_focus.is_focused(window) {
            let byte = if !self.state.generator_name_selection.is_empty() {
                if delta < 0 {
                    self.state.generator_name_selection.start
                } else {
                    self.state.generator_name_selection.end
                }
            } else if delta < 0 {
                self.state.generator_name[..self.state.generator_name_selection.start]
                    .char_indices()
                    .next_back()
                    .map_or(0, |(byte, _)| byte)
            } else {
                self.state.generator_name_selection.end
                    + self.state.generator_name[self.state.generator_name_selection.end..]
                        .chars()
                        .next()
                        .map_or(0, char::len_utf8)
            };
            self.state.generator_name_selection = byte..byte;
            self.state.generator_name_cursor = byte;
            self.state.generator_name_anchor = None;
            self.state.generator_name_marked = None;
            cx.notify();
            return;
        }
        if !self.state.search_focus.is_focused(window) {
            return;
        }
        let byte = if !self.state.selection.is_empty() {
            if delta < 0 {
                self.state.selection.start
            } else {
                self.state.selection.end
            }
        } else if delta < 0 {
            self.state.query[..self.state.selection.start]
                .char_indices()
                .next_back()
                .map_or(0, |(byte, _)| byte)
        } else {
            self.state.selection.end
                + self.state.query[self.state.selection.end..]
                    .chars()
                    .next()
                    .map_or(0, char::len_utf8)
        };
        self.state.selection = byte..byte;
        self.state.marked = None;
        cx.notify();
    }

    pub(crate) fn move_edge(&mut self, end: bool, window: &Window, cx: &mut Context<Self>) {
        if self.state.custom_entry_name_focus.is_focused(window) {
            let byte = if end {
                self.state.custom_entry_name.len()
            } else {
                0
            };
            self.state.custom_entry_name_selection = byte..byte;
            self.state.custom_entry_name_cursor = byte;
            self.state.custom_entry_name_anchor = None;
            cx.notify();
            return;
        }
        if self.state.generator_name_focus.is_focused(window) {
            let byte = if self.text_editor_active() {
                let cursor = self.state.generator_name_cursor;
                if end {
                    self.state.generator_name[cursor..]
                        .find('\n')
                        .map_or(self.state.generator_name.len(), |offset| cursor + offset)
                } else {
                    self.state.generator_name[..cursor]
                        .rfind('\n')
                        .map_or(0, |offset| offset + 1)
                }
            } else if end {
                self.state.generator_name.len()
            } else {
                0
            };
            self.state.generator_name_selection = byte..byte;
            self.state.generator_name_cursor = byte;
            self.state.generator_name_anchor = None;
            self.state.generator_name_marked = None;
            cx.notify();
            return;
        }
        if !self.state.search_focus.is_focused(window) {
            return;
        }
        let byte = if end { self.state.query.len() } else { 0 };
        self.state.selection = byte..byte;
        self.state.marked = None;
        cx.notify();
    }

    pub(crate) fn move_vertical(&mut self, delta: isize, window: &Window, cx: &mut Context<Self>) {
        if !self.state.generator_name_focus.is_focused(window) || !self.text_editor_active() {
            return;
        }
        let cursor = self.state.generator_name_cursor;
        let line_start = self.state.generator_name[..cursor]
            .rfind('\n')
            .map_or(0, |index| index + 1);
        let column = self.state.generator_name[line_start..cursor]
            .chars()
            .count();
        let current_line = self.state.generator_name[..line_start]
            .matches('\n')
            .count();
        let target_line = (current_line as isize + delta).max(0) as usize;
        let mut lines = self.state.generator_name.split('\n');
        let Some(target) = lines.nth(target_line) else {
            return;
        };
        let target_start = self
            .state
            .generator_name
            .split('\n')
            .take(target_line)
            .map(|line| line.len() + 1)
            .sum::<usize>();
        let byte = target_start
            + target
                .char_indices()
                .nth(column)
                .map_or(target.len(), |(byte, _)| byte);
        self.state.generator_name_selection = byte..byte;
        self.state.generator_name_cursor = byte;
        self.state.generator_name_anchor = None;
        self.state.generator_name_marked = None;
        cx.notify();
    }

    pub(crate) fn select_edit_text(
        &mut self,
        delta: isize,
        vertical: bool,
        cx: &mut Context<Self>,
    ) {
        let cursor = self.state.generator_name_cursor;
        let anchor = *self.state.generator_name_anchor.get_or_insert(cursor);
        let next = if vertical {
            let line_start = self.state.generator_name[..cursor]
                .rfind('\n')
                .map_or(0, |i| i + 1);
            let column = self.state.generator_name[line_start..cursor]
                .chars()
                .count();
            let line = self.state.generator_name[..line_start]
                .matches('\n')
                .count() as isize;
            let target_line = (line + delta)
                .clamp(0, self.state.generator_name.matches('\n').count() as isize)
                as usize;
            let target_start = self
                .state
                .generator_name
                .split('\n')
                .take(target_line)
                .map(|s| s.len() + 1)
                .sum::<usize>();
            self.state
                .generator_name
                .split('\n')
                .nth(target_line)
                .map_or(self.state.generator_name.len(), |s| {
                    target_start + s.char_indices().nth(column).map_or(s.len(), |(i, _)| i)
                })
        } else if delta < 0 {
            self.state.generator_name[..cursor]
                .char_indices()
                .next_back()
                .map_or(0, |(i, _)| i)
        } else {
            cursor
                + self.state.generator_name[cursor..]
                    .chars()
                    .next()
                    .map_or(0, char::len_utf8)
        };
        self.state.generator_name_selection = anchor.min(next)..anchor.max(next);
        self.state.generator_name_cursor = next;
        cx.notify();
    }

    pub(super) fn edit_mouse_position(&self, point: Point<Pixels>) -> Option<usize> {
        self.state
            .editor_layout
            .as_ref()
            .map(|layout| layout.index_for_point(point))
    }

    pub(crate) fn begin_edit_mouse_selection(
        &mut self,
        point: Point<Pixels>,
        click_count: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state.generator_name_focus.focus(window);
        if let Some(byte) = self.edit_mouse_position(point) {
            if click_count >= 2 {
                let chars: Vec<_> = self.state.generator_name.char_indices().collect();
                let index = chars
                    .iter()
                    .position(|(start, _)| *start >= byte)
                    .unwrap_or_else(|| chars.len().saturating_sub(1));
                if let Some((_, character)) = chars.get(index) {
                    let is_word = |c: char| c.is_alphanumeric() || c == '_';
                    if is_word(*character) {
                        let mut first = index;
                        while first > 0 && is_word(chars[first - 1].1) {
                            first -= 1;
                        }
                        let mut last = index + 1;
                        while last < chars.len() && is_word(chars[last].1) {
                            last += 1;
                        }
                        let start = chars[first].0;
                        let end = chars
                            .get(last)
                            .map_or(self.state.generator_name.len(), |(start, _)| *start);
                        self.state.generator_name_anchor = Some(start);
                        self.state.generator_name_cursor = end;
                        self.state.generator_name_selection = start..end;
                        cx.notify();
                        return;
                    }
                }
            }
            self.state.generator_name_anchor = Some(byte);
            self.state.generator_name_cursor = byte;
            self.state.generator_name_selection = byte..byte;
            cx.notify();
        }
    }

    pub(crate) fn drag_edit_mouse_selection(
        &mut self,
        point: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if let (Some(anchor), Some(byte)) = (
            self.state.generator_name_anchor,
            self.edit_mouse_position(point),
        ) {
            self.state.generator_name_selection = anchor.min(byte)..anchor.max(byte);
            self.state.generator_name_cursor = byte;
            cx.notify();
        }
    }

    pub(crate) fn end_edit_mouse_selection(&mut self) {
        self.state.generator_name_anchor = None;
    }

    pub(crate) fn copy_edit_selection(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.text_editor_active()
            && self.state.generator_name_focus.is_focused(window)
            && !self.state.generator_name_selection.is_empty()
        {
            let text =
                self.state.generator_name[self.state.generator_name_selection.clone()].to_owned();
            let source = self
                .state
                .session
                .selected()
                .map(|entry| format!("{} (selection)", entry.name()))
                .unwrap_or_else(|| "selection".into());
            self.copy_to_clipboard(text, source, cx);
        }
    }

    pub(crate) fn cut_edit_selection(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.text_editor_active()
            && self.state.generator_name_focus.is_focused(window)
            && !self.state.generator_name_selection.is_empty()
        {
            self.copy_edit_selection(window, cx);
            self.replace_generator_name(None, "", cx);
        }
    }

    pub(crate) fn paste_edit_text(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.state.custom_entry_name_focus.is_focused(window) {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                self.replace_custom_entry_name(None, &text, cx);
            }
            return;
        }
        if self.text_editor_active() && self.state.generator_name_focus.is_focused(window) {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                let text = zeroize::Zeroizing::new(text);
                self.replace_generator_name(None, &text, cx);
            }
        }
    }
}
