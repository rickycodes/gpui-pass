use crate::PassApp;
use gpui::{
    Bounds, Context, ElementInputHandler, EntityInputHandler, Pixels, Point, UTF16Selection,
    Window, canvas, div, prelude::*, px, rgb,
};
use std::ops::Range;

const ADVANCE: f32 = 7.8;

fn byte_at_utf16(text: &str, offset: usize) -> usize {
    let mut utf16 = 0;
    for (byte, character) in text.char_indices() {
        if utf16 + character.len_utf16() > offset {
            return byte;
        }
        utf16 += character.len_utf16();
    }
    text.len()
}

fn utf16_at_byte(text: &str, offset: usize) -> usize {
    text[..offset].encode_utf16().count()
}

impl PassApp {
    fn replace_generator_name(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        cx: &mut Context<Self>,
    ) -> Range<usize> {
        let range = range
            .map(|range| {
                byte_at_utf16(self.generator_name.as_str(), range.start)
                    ..byte_at_utf16(self.generator_name.as_str(), range.end)
            })
            .or_else(|| self.generator_name_marked.clone())
            .unwrap_or_else(|| self.generator_name_selection.clone());
        let start = range.start.min(range.end);
        let end = range.end.max(range.start);
        let text = if self.editing_entry {
            zeroize::Zeroizing::new(text.to_owned())
        } else {
            let room = 512_usize.saturating_sub(
                self.generator_name[..start].chars().count() + self.generator_name[end..].chars().count(),
            );
            zeroize::Zeroizing::new(text.chars().filter(|c| !c.is_control()).take(room).collect::<String>())
        };
        if self.editing_entry {
            let Some(next) = crate::editor_input::replace(&self.generator_name, start..end, &text) else {
                self.set_message(crate::editor_input::EDIT_LIMIT_MESSAGE.into());
                cx.notify();
                return start..start;
            };
            self.generator_name = next;
            self.editor_layout = None;
        } else {
            self.generator_name.replace_range(start..end, &text);
        }
        let inserted = start..start + text.len();
        self.generator_name_selection = inserted.end..inserted.end;
        self.generator_name_cursor = inserted.end;
        self.generator_name_anchor = None;
        self.generator_name_marked = None;
        self.clear_message();
        cx.notify();
        inserted
    }

    pub(crate) fn generator_name_field(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if self.editing_entry {
            let entity = cx.entity().clone();
            let focused = self.generator_name_focus.is_focused(window);
            return div()
                .id("entry-editor-field")
                .track_focus(&self.generator_name_focus)
                .rounded(px(self.theme.radii.field))
                .w_full()
                .min_w_0()
                .flex()
                .items_start()
                .border_1()
                .border_color(rgb(if focused { self.theme.colors.accent } else { self.theme.colors.border }))
                .bg(rgb(self.theme.colors.background))
                .cursor_text()
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|app, event: &gpui::MouseDownEvent, window, cx| {
                        app.begin_edit_mouse_selection(event.position, event.click_count, window, cx);
                    }),
                )
                .on_mouse_move(cx.listener(|app, event: &gpui::MouseMoveEvent, _, cx| {
                    if event.dragging() {
                        app.drag_edit_mouse_selection(event.position, cx);
                    }
                }))
                .on_mouse_up(gpui::MouseButton::Left, cx.listener(|app, _, _, _| app.end_edit_mouse_selection()))
                .on_mouse_up_out(gpui::MouseButton::Left, cx.listener(|app, _, _, _| app.end_edit_mouse_selection()))
                .child(
                    div()
                        .id("entry-editor-scroll")
                        .w_full()
                        .min_w_0()
                        .overflow_x_scroll()
                        .font_family(self.theme.mono_font)
                        .text_size(px(18.))
                        .line_height(px(26.))
                        .text_color(rgb(self.theme.colors.ink))
                        .child(crate::entry_editor::EntryTextElement { app: entity }),
                )
                .into_any_element();
        }
        let focus = self.generator_name_focus.clone();
        let entity = cx.entity().clone();
        let focused = focus.is_focused(window);
        let start = self
            .generator_name_selection
            .start
            .min(self.generator_name.len());
        let end = self
            .generator_name_selection
            .end
            .min(self.generator_name.len());
        let mut text = div()
            .flex()
            .items_center()
            .whitespace_nowrap()
            .font_family(self.theme.mono_font)
            .text_size(px(13.));
        if self.generator_name.is_empty() {
            text = text
                .when(focused, |view| view.child(div().w(px(1.)).h(px(16.)).bg(rgb(self.theme.colors.accent))))
                .child(div().text_color(rgb(self.theme.colors.muted)).child("e.g. Email/example.com"));
        } else {
            text = text
                .child(self.generator_name[..start].to_owned())
                .when(start != end, |view| view.child(div().bg(rgb(self.theme.colors.accent_surface)).child(self.generator_name[start..end].to_owned())))
                .when(focused && start == end, |view| view.child(div().w(px(1.)).h(px(16.)).bg(rgb(self.theme.colors.accent))))
                .child(self.generator_name[end..].to_owned());
        }
        div()
            .id("generator-entry-name")
            .track_focus(&self.generator_name_focus)
            .rounded(px(self.theme.radii.field))
            .h(px(44.))
            .w_full()
            .min_w_0()
            .flex()
            .items_center()
            .px(px(12.))
            .border_1()
            .border_color(rgb(if focused { self.theme.colors.accent } else { self.theme.colors.border }))
            .bg(rgb(self.theme.colors.surface))
            .cursor_text()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|app, event: &gpui::MouseDownEvent, window, cx| {
                    app.generator_name_focus.focus(window);
                    if let Some(bounds) = app.generator_name_bounds {
                        let column = ((event.position.x - bounds.origin.x) / px(ADVANCE)).max(0.) as usize;
                        let byte = app.generator_name.char_indices().nth(column).map_or(app.generator_name.len(), |(byte, _)| byte);
                        app.generator_name_selection = byte..byte;
                        app.generator_name_cursor = byte;
                    }
                    cx.notify();
                }),
            )
            .child(
                div()
                    .id("generator-entry-name-input")
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h(px(20.))
                    .overflow_hidden()
                    .child(text)
                    .child(
                        canvas(
                            |_, _, _| (),
                            move |bounds, (), window, cx| {
                                entity
                                    .update(cx, |app, _| app.generator_name_bounds = Some(bounds));
                                if focus.is_focused(window) {
                                    window.handle_input(
                                        &focus,
                                        ElementInputHandler::new(bounds, entity.clone()),
                                        cx,
                                    );
                                }
                            },
                        )
                        .absolute()
                        .size_full(),
                    ),
            )
            .into_any_element()
    }

    pub(crate) fn search_field(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let focus = self.search_focus.clone();
        let entity = cx.entity();
        let focused = self.search_focus.is_focused(window);
        let start = self.selection.start.min(self.query.len());
        let end = self.selection.end.min(self.query.len());
        let mut text = div()
            .flex()
            .items_center()
            .whitespace_nowrap()
            .font_family(self.theme.mono_font)
            .text_size(px(13.));
        if self.query.is_empty() {
            text = text
                .when(focused, |view| {
                    view.child(div().w(px(1.)).h(px(16.)).bg(rgb(self.theme.colors.accent)))
                })
                .child(div().text_color(rgb(self.theme.colors.muted)).child("Search entries"));
        } else {
            text = text
                .child(self.query[..start].to_owned())
                .when(start != end, |view| {
                    view.child(div().bg(rgb(self.theme.colors.accent_surface)).child(self.query[start..end].to_owned()))
                })
                .when(focused && start == end, |view| {
                    view.child(div().w(px(1.)).h(px(16.)).bg(rgb(self.theme.colors.accent)))
                })
                .child(self.query[end..].to_owned());
        }
        div()
            .id("search-field")
            .track_focus(&self.search_focus)
            .rounded(px(self.theme.radii.field))
            .h(px(42.))
            .w_full()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(12.))
            .border_1()
            .border_color(rgb(if focused { self.theme.colors.accent } else { self.theme.colors.border }))
            .bg(rgb(self.theme.colors.surface))
            .cursor_text()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|app, event: &gpui::MouseDownEvent, window, cx| {
                    app.search_focus.focus(window);
                    if let Some(bounds) = app.input_bounds {
                        let column =
                            ((event.position.x - bounds.origin.x) / px(ADVANCE)).max(0.) as usize;
                        let byte = app
                            .query
                            .char_indices()
                            .nth(column)
                            .map_or(app.query.len(), |(byte, _)| byte);
                        app.selection = byte..byte;
                    }
                    cx.notify();
                }),
            )
            .child(div().text_size(px(16.)).text_color(rgb(self.theme.colors.muted)).child("⌕"))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h(px(20.))
                    .overflow_hidden()
                    .child(text)
                    .child(
                        canvas(
                            |_, _, _| (),
                            move |bounds, (), window, cx| {
                                entity.update(cx, |app, _| app.input_bounds = Some(bounds));
                                if focus.is_focused(window) {
                                    window.handle_input(
                                        &focus,
                                        ElementInputHandler::new(bounds, entity.clone()),
                                        cx,
                                    );
                                }
                            },
                        )
                        .absolute()
                        .size_full(),
                    ),
            )
            .into_any_element()
    }

    fn replace_search(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        cx: &mut Context<Self>,
    ) -> Range<usize> {
        let range = range
            .map(|range| {
                byte_at_utf16(self.query.as_str(), range.start)..byte_at_utf16(self.query.as_str(), range.end)
            })
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selection.clone());
        let start = range.start.min(range.end);
        let end = range.end.max(range.start);
        let room = 512_usize.saturating_sub(
            self.query[..start].chars().count() + self.query[end..].chars().count(),
        );
        let text: String = text
            .chars()
            .filter(|c| !c.is_control())
            .take(room)
            .collect();
        self.query.replace_range(start..end, &text);
        let inserted = start..start + text.len();
        self.selection = inserted.end..inserted.end;
        self.marked = None;
        self.filter(cx);
        cx.notify();
        inserted
    }

    pub(crate) fn backspace(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.generator_name_focus.is_focused(window) {
            if self.generator_name_selection.is_empty() {
                self.generator_name_selection.start = self.generator_name
                    [..self.generator_name_selection.start]
                    .char_indices()
                    .next_back()
                    .map_or(0, |(byte, _)| byte);
            }
            self.replace_generator_name(None, "", cx);
            return;
        }
        if !self.search_focus.is_focused(window) {
            return;
        }
        if self.selection.is_empty() {
            let previous = self.query[..self.selection.start]
                .char_indices()
                .next_back()
                .map_or(0, |(byte, _)| byte);
            self.selection.start = previous;
        }
        self.replace_search(None, "", cx);
    }

    pub(crate) fn delete(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.generator_name_focus.is_focused(window) {
            if self.generator_name_selection.is_empty() {
                self.generator_name_selection.end += self.generator_name
                    [self.generator_name_selection.end..]
                    .chars()
                    .next()
                    .map_or(0, char::len_utf8);
            }
            self.replace_generator_name(None, "", cx);
            return;
        }
        if !self.search_focus.is_focused(window) {
            return;
        }
        if self.selection.is_empty() {
            self.selection.end += self.query[self.selection.end..]
                .chars()
                .next()
                .map_or(0, char::len_utf8);
        }
        self.replace_search(None, "", cx);
    }

    pub(crate) fn move_cursor(&mut self, delta: isize, window: &Window, cx: &mut Context<Self>) {
        if self.generator_name_focus.is_focused(window) {
            let byte = if !self.generator_name_selection.is_empty() {
                if delta < 0 {
                    self.generator_name_selection.start
                } else {
                    self.generator_name_selection.end
                }
            } else if delta < 0 {
                self.generator_name[..self.generator_name_selection.start]
                    .char_indices()
                    .next_back()
                    .map_or(0, |(byte, _)| byte)
            } else {
                self.generator_name_selection.end
                    + self.generator_name[self.generator_name_selection.end..]
                        .chars()
                        .next()
                        .map_or(0, char::len_utf8)
            };
            self.generator_name_selection = byte..byte;
            self.generator_name_cursor = byte;
            self.generator_name_anchor = None;
            self.generator_name_marked = None;
            cx.notify();
            return;
        }
        if !self.search_focus.is_focused(window) {
            return;
        }
        let byte = if !self.selection.is_empty() {
            if delta < 0 {
                self.selection.start
            } else {
                self.selection.end
            }
        } else if delta < 0 {
            self.query[..self.selection.start]
                .char_indices()
                .next_back()
                .map_or(0, |(byte, _)| byte)
        } else {
            self.selection.end
                + self.query[self.selection.end..]
                    .chars()
                    .next()
                    .map_or(0, char::len_utf8)
        };
        self.selection = byte..byte;
        self.marked = None;
        cx.notify();
    }

    pub(crate) fn move_edge(&mut self, end: bool, window: &Window, cx: &mut Context<Self>) {
        if self.generator_name_focus.is_focused(window) {
            let byte = if self.editing_entry {
                let cursor = self.generator_name_cursor;
                if end {
                    self.generator_name[cursor..]
                        .find('\n')
                        .map_or(self.generator_name.len(), |offset| cursor + offset)
                } else {
                    self.generator_name[..cursor]
                        .rfind('\n')
                        .map_or(0, |offset| offset + 1)
                }
            } else if end {
                self.generator_name.len()
            } else {
                0
            };
            self.generator_name_selection = byte..byte;
            self.generator_name_cursor = byte;
            self.generator_name_anchor = None;
            self.generator_name_marked = None;
            cx.notify();
            return;
        }
        if !self.search_focus.is_focused(window) {
            return;
        }
        let byte = if end { self.query.len() } else { 0 };
        self.selection = byte..byte;
        self.marked = None;
        cx.notify();
    }

    pub(crate) fn move_vertical(&mut self, delta: isize, window: &Window, cx: &mut Context<Self>) {
        if !self.generator_name_focus.is_focused(window) || !self.editing_entry {
            return;
        }
        let cursor = self.generator_name_cursor;
        let line_start = self.generator_name[..cursor]
            .rfind('\n')
            .map_or(0, |index| index + 1);
        let column = self.generator_name[line_start..cursor].chars().count();
        let current_line = self.generator_name[..line_start].matches('\n').count();
        let target_line = (current_line as isize + delta).max(0) as usize;
        let mut lines = self.generator_name.split('\n');
        let Some(target) = lines.nth(target_line) else { return; };
        let target_start = self.generator_name
            .split('\n')
            .take(target_line)
            .map(|line| line.len() + 1)
            .sum::<usize>();
        let byte = target_start
            + target.char_indices().nth(column).map_or(target.len(), |(byte, _)| byte);
        self.generator_name_selection = byte..byte;
        self.generator_name_cursor = byte;
        self.generator_name_anchor = None;
        self.generator_name_marked = None;
        cx.notify();
    }

    pub(crate) fn select_edit_text(&mut self, delta: isize, vertical: bool, cx: &mut Context<Self>) {
        let cursor = self.generator_name_cursor;
        let anchor = *self.generator_name_anchor.get_or_insert(cursor);
        let next = if vertical {
            let line_start = self.generator_name[..cursor].rfind('\n').map_or(0, |i| i + 1);
            let column = self.generator_name[line_start..cursor].chars().count();
            let line = self.generator_name[..line_start].matches('\n').count() as isize;
            let target_line = (line + delta).clamp(0, self.generator_name.matches('\n').count() as isize) as usize;
            let target_start = self.generator_name.split('\n').take(target_line).map(|s| s.len() + 1).sum::<usize>();
            self.generator_name.split('\n').nth(target_line).map_or(self.generator_name.len(), |s| {
                target_start + s.char_indices().nth(column).map_or(s.len(), |(i, _)| i)
            })
        } else if delta < 0 {
            self.generator_name[..cursor].char_indices().next_back().map_or(0, |(i, _)| i)
        } else {
            cursor + self.generator_name[cursor..].chars().next().map_or(0, char::len_utf8)
        };
        self.generator_name_selection = anchor.min(next)..anchor.max(next);
        self.generator_name_cursor = next;
        cx.notify();
    }

    fn edit_mouse_position(&self, point: Point<Pixels>) -> Option<usize> {
        self.editor_layout.as_ref().map(|layout| layout.index_for_point(point))
    }

    pub(crate) fn begin_edit_mouse_selection(
        &mut self,
        point: Point<Pixels>,
        click_count: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.generator_name_focus.focus(window);
        if let Some(byte) = self.edit_mouse_position(point) {
            if click_count >= 2 {
                let chars: Vec<_> = self.generator_name.char_indices().collect();
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
                        let end = chars.get(last).map_or(self.generator_name.len(), |(start, _)| *start);
                        self.generator_name_anchor = Some(start);
                        self.generator_name_cursor = end;
                        self.generator_name_selection = start..end;
                        cx.notify();
                        return;
                    }
                }
            }
            self.generator_name_anchor = Some(byte);
            self.generator_name_cursor = byte;
            self.generator_name_selection = byte..byte;
            cx.notify();
        }
    }

    pub(crate) fn drag_edit_mouse_selection(&mut self, point: Point<Pixels>, cx: &mut Context<Self>) {
        if let (Some(anchor), Some(byte)) = (self.generator_name_anchor, self.edit_mouse_position(point)) {
            self.generator_name_selection = anchor.min(byte)..anchor.max(byte);
            self.generator_name_cursor = byte;
            cx.notify();
        }
    }

    pub(crate) fn end_edit_mouse_selection(&mut self) {
        self.generator_name_anchor = None;
    }

    pub(crate) fn copy_edit_selection(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.editing_entry && self.generator_name_focus.is_focused(window)
            && !self.generator_name_selection.is_empty()
        {
            let text = self.generator_name[self.generator_name_selection.clone()].to_owned();
            let source = self.session.selected().map(|entry| format!("{} (selection)", entry.name())).unwrap_or_else(|| "selection".into());
            self.copy_to_clipboard(text, source, cx);
        }
    }

    pub(crate) fn cut_edit_selection(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.editing_entry && self.generator_name_focus.is_focused(window)
            && !self.generator_name_selection.is_empty()
        {
            self.copy_edit_selection(window, cx);
            self.replace_generator_name(None, "", cx);
        }
    }

    pub(crate) fn paste_edit_text(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.editing_entry && self.generator_name_focus.is_focused(window) {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                let text = zeroize::Zeroizing::new(text);
                self.replace_generator_name(None, &text, cx);
            }
        }
    }
}

impl EntityInputHandler for PassApp {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        window: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        if self.generator_name_focus.is_focused(window) {
            let start = byte_at_utf16(self.generator_name.as_str(), range.start);
            let end = byte_at_utf16(self.generator_name.as_str(), range.end).max(start);
            *adjusted = Some(
                utf16_at_byte(self.generator_name.as_str(), start)
                    ..utf16_at_byte(self.generator_name.as_str(), end),
            );
            return Some(self.generator_name[start..end].to_owned());
        }
        let start = byte_at_utf16(self.query.as_str(), range.start);
        let end = byte_at_utf16(self.query.as_str(), range.end).max(start);
        *adjusted = Some(utf16_at_byte(self.query.as_str(), start)..utf16_at_byte(self.query.as_str(), end));
        Some(self.query[start..end].to_owned())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        window: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let (text, selection) = if self.generator_name_focus.is_focused(window) {
            (self.generator_name.as_str(), &self.generator_name_selection)
        } else {
            (self.query.as_str(), &self.selection)
        };
        Some(UTF16Selection {
            range: utf16_at_byte(text, selection.start)..utf16_at_byte(text, selection.end),
            reversed: self.generator_name_focus.is_focused(window)
                && self.generator_name_cursor < selection.start,
        })
    }

    fn marked_text_range(
        &self,
        window: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        let (text, marked) = if self.generator_name_focus.is_focused(window) {
            (self.generator_name.as_str(), &self.generator_name_marked)
        } else {
            (self.query.as_str(), &self.marked)
        };
        marked
            .as_ref()
            .map(|range| utf16_at_byte(text, range.start)..utf16_at_byte(text, range.end))
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.generator_name_focus.is_focused(window) {
            self.generator_name_marked = None;
        } else {
            self.marked = None;
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
        if self.generator_name_focus.is_focused(window) {
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
        let name_focused = self.generator_name_focus.is_focused(window);
        let inserted = if name_focused {
            self.replace_generator_name(range, text, cx)
        } else {
            self.replace_search(range, text, cx)
        };
        let (source, marked) = if name_focused {
            (self.generator_name.as_str(), &mut self.generator_name_marked)
        } else {
            (self.query.as_str(), &mut self.marked)
        };
        *marked = (!inserted.is_empty()).then_some(inserted.clone());
        if let Some(selected) = selected {
            let text = &source[inserted.clone()];
            let selection = inserted.start + byte_at_utf16(text, selected.start)
                ..inserted.start + byte_at_utf16(text, selected.end);
            if name_focused {
                self.generator_name_cursor = selection.end;
                self.generator_name_selection = selection;
            } else {
                self.selection = selection;
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
        let name_focused = self.generator_name_focus.is_focused(window);
        let text = if name_focused {
            self.generator_name.as_str()
        } else {
            self.query.as_str()
        };
        let byte = byte_at_utf16(text, range.start);
        if name_focused && self.editing_entry {
            let line = self.editor_layout.as_ref()?.lines.iter()
                .find(|line| byte >= line.range.start && byte <= line.range.end)
                .or_else(|| self.editor_layout.as_ref()?.lines.last())?;
            let offset = byte.saturating_sub(line.range.start).min(line.range.end - line.range.start);
            return Some(Bounds::new(
                Point { x: line.bounds.left() + line.shaped.x_for_index(offset), y: line.bounds.top() },
                gpui::size(px(1.), line.bounds.size.height),
            ));
        }
        let (column, row) = if self.generator_name_focus.is_focused(window) && self.editing_entry {
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
                if self.generator_name_focus.is_focused(window) && self.editing_entry {
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
        let name_focused = self.generator_name_focus.is_focused(window);
        let text = if name_focused { self.generator_name.as_str() } else { self.query.as_str() };
        if name_focused && self.editing_entry {
            let byte = self.editor_layout.as_ref()?.index_for_point(point);
            return Some(utf16_at_byte(text, byte));
        }
        let bounds = if name_focused {
            self.generator_name_bounds?
        } else {
            self.input_bounds?
        };
        let column = ((point.x - bounds.origin.x) / px(ADVANCE)).max(0.) as usize;
        Some(text.chars().take(column).map(char::len_utf16).sum())
    }
}
