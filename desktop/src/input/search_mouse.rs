use super::*;

impl PassApp {
    pub(crate) fn edit_mouse_position(&self, point: Point<Pixels>) -> Option<usize> {
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
        self.state.composer_content_focus.focus(window);
        if let Some(byte) = self.edit_mouse_position(point) {
            if click_count >= 2 {
                let chars: Vec<_> = self.state.composer_content.char_indices().collect();
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
                            .map_or(self.state.composer_content.len(), |(start, _)| *start);
                        self.state.composer_content_anchor = Some(start);
                        self.state.composer_content_cursor = end;
                        self.state.composer_content_selection = start..end;
                        cx.notify();
                        return;
                    }
                }
            }
            self.state.composer_content_anchor = Some(byte);
            self.state.composer_content_cursor = byte;
            self.state.composer_content_selection = byte..byte;
            cx.notify();
        }
    }

    pub(crate) fn drag_edit_mouse_selection(
        &mut self,
        point: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if let (Some(anchor), Some(byte)) = (
            self.state.composer_content_anchor,
            self.edit_mouse_position(point),
        ) {
            self.state.composer_content_selection = anchor.min(byte)..anchor.max(byte);
            self.state.composer_content_cursor = byte;
            cx.notify();
        }
    }

    pub(crate) fn end_edit_mouse_selection(&mut self) {
        self.state.composer_content_anchor = None;
    }

    pub(crate) fn copy_edit_selection(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.text_editor_active()
            && self.state.composer_content_focus.is_focused(window)
            && !self.state.composer_content_selection.is_empty()
        {
            let text = self.state.composer_content[self.state.composer_content_selection.clone()]
                .to_owned();
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
            && self.state.composer_content_focus.is_focused(window)
            && !self.state.composer_content_selection.is_empty()
        {
            self.copy_edit_selection(window, cx);
            self.replace_composer_content(None, "", cx);
        }
    }

    pub(crate) fn paste_edit_text(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.state.custom_entry_name_focus.is_focused(window) {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                self.replace_custom_entry_name(None, &text, cx);
            }
            return;
        }
        if self.text_editor_active() && self.state.composer_content_focus.is_focused(window) {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                let text = zeroize::Zeroizing::new(text);
                self.replace_composer_content(None, &text, cx);
            }
        }
    }
}
