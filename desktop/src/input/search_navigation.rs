use super::*;

impl PassApp {
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
        if self.state.composer_content_focus.is_focused(window) {
            if self.state.composer_content_selection.is_empty() {
                self.state.composer_content_selection.start = self.state.composer_content
                    [..self.state.composer_content_selection.start]
                    .char_indices()
                    .next_back()
                    .map_or(0, |(byte, _)| byte);
            }
            self.replace_composer_content(None, "", cx);
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
        if self.state.composer_content_focus.is_focused(window) {
            if self.state.composer_content_selection.is_empty() {
                self.state.composer_content_selection.end += self.state.composer_content
                    [self.state.composer_content_selection.end..]
                    .chars()
                    .next()
                    .map_or(0, char::len_utf8);
            }
            self.replace_composer_content(None, "", cx);
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
        if self.state.composer_content_focus.is_focused(window) {
            let byte = if !self.state.composer_content_selection.is_empty() {
                if delta < 0 {
                    self.state.composer_content_selection.start
                } else {
                    self.state.composer_content_selection.end
                }
            } else if delta < 0 {
                self.state.composer_content[..self.state.composer_content_selection.start]
                    .char_indices()
                    .next_back()
                    .map_or(0, |(byte, _)| byte)
            } else {
                self.state.composer_content_selection.end
                    + self.state.composer_content[self.state.composer_content_selection.end..]
                        .chars()
                        .next()
                        .map_or(0, char::len_utf8)
            };
            self.state.composer_content_selection = byte..byte;
            self.state.composer_content_cursor = byte;
            self.state.composer_content_anchor = None;
            self.state.composer_content_marked = None;
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
        if self.state.composer_content_focus.is_focused(window) {
            let byte = if self.text_editor_active() {
                let cursor = self.state.composer_content_cursor;
                if end {
                    self.state.composer_content[cursor..]
                        .find('\n')
                        .map_or(self.state.composer_content.len(), |offset| cursor + offset)
                } else {
                    self.state.composer_content[..cursor]
                        .rfind('\n')
                        .map_or(0, |offset| offset + 1)
                }
            } else if end {
                self.state.composer_content.len()
            } else {
                0
            };
            self.state.composer_content_selection = byte..byte;
            self.state.composer_content_cursor = byte;
            self.state.composer_content_anchor = None;
            self.state.composer_content_marked = None;
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
        if !self.state.composer_content_focus.is_focused(window) || !self.text_editor_active() {
            return;
        }
        let cursor = self.state.composer_content_cursor;
        let line_start = self.state.composer_content[..cursor]
            .rfind('\n')
            .map_or(0, |index| index + 1);
        let column = self.state.composer_content[line_start..cursor]
            .chars()
            .count();
        let current_line = self.state.composer_content[..line_start]
            .matches('\n')
            .count();
        let target_line = (current_line as isize + delta).max(0) as usize;
        let mut lines = self.state.composer_content.split('\n');
        let Some(target) = lines.nth(target_line) else {
            return;
        };
        let target_start = self
            .state
            .composer_content
            .split('\n')
            .take(target_line)
            .map(|line| line.len() + 1)
            .sum::<usize>();
        let byte = target_start
            + target
                .char_indices()
                .nth(column)
                .map_or(target.len(), |(byte, _)| byte);
        self.state.composer_content_selection = byte..byte;
        self.state.composer_content_cursor = byte;
        self.state.composer_content_anchor = None;
        self.state.composer_content_marked = None;
        cx.notify();
    }

    pub(crate) fn select_edit_text(
        &mut self,
        delta: isize,
        vertical: bool,
        cx: &mut Context<Self>,
    ) {
        let cursor = self.state.composer_content_cursor;
        let anchor = *self.state.composer_content_anchor.get_or_insert(cursor);
        let next = if vertical {
            let line_start = self.state.composer_content[..cursor]
                .rfind('\n')
                .map_or(0, |i| i + 1);
            let column = self.state.composer_content[line_start..cursor]
                .chars()
                .count();
            let line = self.state.composer_content[..line_start]
                .matches('\n')
                .count() as isize;
            let target_line = (line + delta).clamp(
                0,
                self.state.composer_content.matches('\n').count() as isize,
            ) as usize;
            let target_start = self
                .state
                .composer_content
                .split('\n')
                .take(target_line)
                .map(|s| s.len() + 1)
                .sum::<usize>();
            self.state
                .composer_content
                .split('\n')
                .nth(target_line)
                .map_or(self.state.composer_content.len(), |s| {
                    target_start + s.char_indices().nth(column).map_or(s.len(), |(i, _)| i)
                })
        } else if delta < 0 {
            self.state.composer_content[..cursor]
                .char_indices()
                .next_back()
                .map_or(0, |(i, _)| i)
        } else {
            cursor
                + self.state.composer_content[cursor..]
                    .chars()
                    .next()
                    .map_or(0, char::len_utf8)
        };
        self.state.composer_content_selection = anchor.min(next)..anchor.max(next);
        self.state.composer_content_cursor = next;
        cx.notify();
    }
}
