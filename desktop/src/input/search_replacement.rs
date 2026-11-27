use super::*;

impl PassApp {
    pub(crate) fn replace_custom_entry_name(
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
        if self.state.onboarding.is_some() {
            self.state.onboarding_recipient_edited = true;
        }
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

    pub(crate) fn replace_composer_content(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        cx: &mut Context<Self>,
    ) -> Range<usize> {
        if self.state.navigation.composer_open() {
            self.state.content_generated = false;
        }
        let range = range
            .map(|range| {
                byte_at_utf16(self.state.composer_content.as_str(), range.start)
                    ..byte_at_utf16(self.state.composer_content.as_str(), range.end)
            })
            .or_else(|| self.state.composer_content_marked.clone())
            .unwrap_or_else(|| self.state.composer_content_selection.clone());
        let start = range.start.min(range.end);
        let end = range.end.max(range.start);
        let text = if self.text_editor_active() {
            zeroize::Zeroizing::new(text.to_owned())
        } else {
            let room = 512_usize.saturating_sub(
                self.state.composer_content[..start].chars().count()
                    + self.state.composer_content[end..].chars().count(),
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
                crate::editor_input::replace(&self.state.composer_content, start..end, &text)
            else {
                self.set_error(crate::i18n::text("editor-limit"));
                cx.notify();
                return start..start;
            };
            self.state.composer_content = next;
            self.state.editor_layout = None;
        } else {
            self.state.composer_content.replace_range(start..end, &text);
        }
        let inserted = start..start + text.len();
        self.state.composer_content_selection = inserted.end..inserted.end;
        self.state.composer_content_cursor = inserted.end;
        self.state.composer_content_anchor = None;
        self.state.composer_content_marked = None;
        self.clear_message();
        cx.notify();
        inserted
    }

    pub(crate) fn replace_search(
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
}
