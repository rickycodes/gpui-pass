use crate::{PassApp, theme::*};
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
            .font_family(MONO)
            .text_size(px(13.));
        if self.query.is_empty() {
            text = text
                .when(focused, |view| {
                    view.child(div().w(px(1.)).h(px(16.)).bg(rgb(GREEN)))
                })
                .child(div().text_color(rgb(MUTED)).child("Search entries"));
        } else {
            text = text
                .child(self.query[..start].to_owned())
                .when(start != end, |view| {
                    view.child(div().bg(rgb(TINT)).child(self.query[start..end].to_owned()))
                })
                .when(focused && start == end, |view| {
                    view.child(div().w(px(1.)).h(px(16.)).bg(rgb(GREEN)))
                })
                .child(self.query[end..].to_owned());
        }
        div()
            .id("search-field")
            .track_focus(&self.search_focus)
            .h(px(42.))
            .w_full()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(12.))
            .border_1()
            .border_color(rgb(if focused { GREEN } else { LINE }))
            .bg(rgb(WHITE))
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
            .child(div().text_size(px(16.)).text_color(rgb(MUTED)).child("⌕"))
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
                byte_at_utf16(&self.query, range.start)..byte_at_utf16(&self.query, range.end)
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
        self.filter();
        cx.notify();
        inserted
    }

    pub(crate) fn backspace(&mut self, window: &Window, cx: &mut Context<Self>) {
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
        if !self.search_focus.is_focused(window) {
            return;
        }
        let byte = if end { self.query.len() } else { 0 };
        self.selection = byte..byte;
        self.marked = None;
        cx.notify();
    }
}

impl EntityInputHandler for PassApp {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let start = byte_at_utf16(&self.query, range.start);
        let end = byte_at_utf16(&self.query, range.end).max(start);
        *adjusted = Some(utf16_at_byte(&self.query, start)..utf16_at_byte(&self.query, end));
        Some(self.query[start..end].to_owned())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: utf16_at_byte(&self.query, self.selection.start)
                ..utf16_at_byte(&self.query, self.selection.end),
            reversed: false,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|range| {
            utf16_at_byte(&self.query, range.start)..utf16_at_byte(&self.query, range.end)
        })
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_search(range, text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let inserted = self.replace_search(range, text, cx);
        self.marked = (!inserted.is_empty()).then_some(inserted.clone());
        if let Some(selected) = selected {
            let text = &self.query[inserted.clone()];
            self.selection = inserted.start + byte_at_utf16(text, selected.start)
                ..inserted.start + byte_at_utf16(text, selected.end);
        }
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let byte = byte_at_utf16(&self.query, range.start);
        let column = self.query[..byte].chars().count() as f32;
        Some(Bounds {
            origin: Point {
                x: bounds.origin.x + px(column * ADVANCE),
                y: bounds.origin.y,
            },
            size: gpui::size(px(1.), bounds.size.height),
        })
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.input_bounds?;
        let column = ((point.x - bounds.origin.x) / px(ADVANCE)).max(0.) as usize;
        Some(self.query.chars().take(column).map(char::len_utf16).sum())
    }
}
