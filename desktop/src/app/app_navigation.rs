use super::*;

impl PassApp {
    pub(crate) fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.state.saving.is_some() {
            return;
        }
        self.cancel_edit(cx);
        self.state
            .session
            .select(self.state.entries.get(index).cloned());
        self.state.reveal = None;
        self.state.copy_pending = None;
        self.clear_message();
        if let Some(position) = self.state.sidebar_rows.iter().position(
            |row| matches!(row, SidebarRow::Entry { index: row_index, .. } if *row_index == index),
        ) {
            self.state.sidebar_cursor = position;
        }
        cx.notify();
    }

    pub(crate) fn navigate(&mut self, delta: isize, cx: &mut Context<Self>) {
        self.state.sidebar_scroll.cancel();
        if self.state.sidebar_rows.is_empty() {
            return;
        }
        let position = self
            .state
            .sidebar_cursor
            .min(self.state.sidebar_rows.len().saturating_sub(1));
        let next = (position as isize + delta).clamp(0, self.state.sidebar_rows.len() as isize - 1)
            as usize;
        self.state.sidebar_cursor = next;
        if let SidebarRow::Entry { index, .. } = self.state.sidebar_rows[next] {
            self.choose(index, cx);
        } else {
            cx.notify();
        }
        self.state
            .list_scroll
            .scroll_to_item(next, ScrollStrategy::Center);
    }

    pub(crate) fn hide(&mut self, cx: &mut Context<Self>) {
        if self.state.confirming_overwrite.take().is_some() {
            cx.notify();
            return;
        }
        if self.state.confirming_delete.take().is_some() {
            cx.notify();
            return;
        }
        if self.state.navigation.composer_open() {
            self.close_composer(cx);
            return;
        }
        if self.state.editing_entry {
            self.cancel_edit(cx);
            cx.notify();
            return;
        }
        if self.state.navigation.page() == Page::CardDetails {
            self.leave_card_status(cx);
            return;
        }
        self.state.session.hide();
        self.state.copy_pending = None;
        self.state.reveal = None;
        self.clear_message();
        cx.notify();
    }
}
