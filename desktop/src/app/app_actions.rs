use super::*;
use gpui::Div;

actions!(
    pass,
    [
        Hide,
        OpenGenerator,
        Reveal,
        Refresh,
        FocusSearch,
        NextEntry,
        PreviousEntry,
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        Copy,
        CopyEntry,
        Cut,
        Paste,
        Start,
        End,
        SelectAll,
        SwitchFocus,
        ToggleFullscreen,
        Quit
    ]
);

pub(super) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", Hide, Some("Pass")),
        KeyBinding::new("ctrl-g", OpenGenerator, Some("Pass")),
        KeyBinding::new("enter", Reveal, Some("Pass")),
        KeyBinding::new("ctrl-r", Refresh, Some("Pass")),
        KeyBinding::new("ctrl-f", FocusSearch, Some("Pass")),
        KeyBinding::new("down", NextEntry, Some("Pass")),
        KeyBinding::new("up", PreviousEntry, Some("Pass")),
        KeyBinding::new("backspace", Backspace, Some("Pass")),
        KeyBinding::new("delete", Delete, Some("Pass")),
        KeyBinding::new("left", Left, Some("Pass")),
        KeyBinding::new("right", Right, Some("Pass")),
        KeyBinding::new("shift-left", SelectLeft, Some("Pass")),
        KeyBinding::new("shift-right", SelectRight, Some("Pass")),
        KeyBinding::new("shift-up", SelectUp, Some("Pass")),
        KeyBinding::new("shift-down", SelectDown, Some("Pass")),
        KeyBinding::new("ctrl-c", Copy, Some("Pass")),
        KeyBinding::new("ctrl-shift-c", CopyEntry, Some("Pass")),
        KeyBinding::new("ctrl-x", Cut, Some("Pass")),
        KeyBinding::new("ctrl-v", Paste, Some("Pass")),
        KeyBinding::new("home", Start, Some("Pass")),
        KeyBinding::new("end", End, Some("Pass")),
        KeyBinding::new("ctrl-a", SelectAll, Some("Pass")),
        KeyBinding::new("tab", SwitchFocus, Some("Pass")),
        KeyBinding::new("shift-tab", SwitchFocus, Some("Pass")),
        KeyBinding::new("ctrl-q", Quit, Some("Pass")),
        KeyBinding::new("f11", ToggleFullscreen, Some("Pass")),
    ]);
}

pub(super) fn bind_actions(view: Div, cx: &mut Context<PassApp>) -> Div {
    view.on_action(cx.listener(|app, _: &Hide, _, cx| app.hide(cx)))
        .on_action(cx.listener(|app, _: &OpenGenerator, window, cx| app.open_generator(window, cx)))
        .on_action(cx.listener(|app, _: &Reveal, window, cx| {
            if !app.state.navigation.generator_open()
                && app.state.confirming_delete.is_none()
                && app.state.confirming_overwrite.is_none()
                && !app.state.generator_name_focus.is_focused(window)
                && !app.state.custom_entry_name_focus.is_focused(window)
            {
                app.reveal(window, cx);
            }
        }))
        .on_action(cx.listener(|app, _: &Refresh, _, cx| app.refresh(cx)))
        .on_action(cx.listener(|app, _: &FocusSearch, window, cx| {
            app.hide(cx);
            app.state.search_focus.focus(window);
            app.state.selection = 0..app.state.query.len();
            cx.notify();
        }))
        .on_action(cx.listener(|app, _: &NextEntry, window, cx| {
            if app.state.custom_entry_name_focus.is_focused(window) {
            } else if app.state.generator_name_focus.is_focused(window) && app.text_editor_active()
            {
                app.move_vertical(1, window, cx);
            } else {
                app.navigate(1, cx);
            }
        }))
        .on_action(cx.listener(|app, _: &PreviousEntry, window, cx| {
            if app.state.custom_entry_name_focus.is_focused(window) {
            } else if app.state.generator_name_focus.is_focused(window) && app.text_editor_active()
            {
                app.move_vertical(-1, window, cx);
            } else {
                app.navigate(-1, cx);
            }
        }))
        .on_action(cx.listener(|app, _: &Backspace, window, cx| app.backspace(window, cx)))
        .on_action(cx.listener(|app, _: &Delete, window, cx| app.delete(window, cx)))
        .on_action(cx.listener(|app, _: &Left, window, cx| app.move_cursor(-1, window, cx)))
        .on_action(cx.listener(|app, _: &Right, window, cx| app.move_cursor(1, window, cx)))
        .on_action(cx.listener(|app, _: &SelectLeft, window, cx| {
            if app.state.custom_entry_name_focus.is_focused(window) {
                app.select_custom_entry_name(-1, cx);
            } else if app.text_editor_active() && app.state.generator_name_focus.is_focused(window)
            {
                app.select_edit_text(-1, false, cx);
            }
        }))
        .on_action(cx.listener(|app, _: &SelectRight, window, cx| {
            if app.state.custom_entry_name_focus.is_focused(window) {
                app.select_custom_entry_name(1, cx);
            } else if app.text_editor_active() && app.state.generator_name_focus.is_focused(window)
            {
                app.select_edit_text(1, false, cx);
            }
        }))
        .on_action(cx.listener(|app, _: &SelectUp, window, cx| {
            if app.text_editor_active() && app.state.generator_name_focus.is_focused(window) {
                app.select_edit_text(-1, true, cx);
            }
        }))
        .on_action(cx.listener(|app, _: &SelectDown, window, cx| {
            if app.text_editor_active() && app.state.generator_name_focus.is_focused(window) {
                app.select_edit_text(1, true, cx);
            }
        }))
        .on_action(cx.listener(|app, _: &Copy, window, cx| app.copy_edit_selection(window, cx)))
        .on_action(cx.listener(|app, _: &CopyEntry, window, cx| {
            if !app.state.navigation.generator_open()
                && !app.state.editing_entry
                && !app.state.generator_name_focus.is_focused(window)
                && !app.state.custom_entry_name_focus.is_focused(window)
                && app.state.confirming_delete.is_none()
                && app.state.confirming_overwrite.is_none()
            {
                app.copy_selected(cx);
            }
        }))
        .on_action(cx.listener(|app, _: &Cut, window, cx| app.cut_edit_selection(window, cx)))
        .on_action(cx.listener(|app, _: &Paste, window, cx| app.paste_edit_text(window, cx)))
        .on_action(cx.listener(|app, _: &Start, window, cx| app.move_edge(false, window, cx)))
        .on_action(cx.listener(|app, _: &End, window, cx| app.move_edge(true, window, cx)))
        .on_action(cx.listener(|app, _: &SelectAll, window, cx| {
            if app.state.custom_entry_name_focus.is_focused(window) {
                app.state.custom_entry_name_selection = 0..app.state.custom_entry_name.len();
                app.state.custom_entry_name_anchor = Some(0);
                app.state.custom_entry_name_cursor = app.state.custom_entry_name.len();
                cx.notify();
            } else if app.state.generator_name_focus.is_focused(window) {
                app.state.generator_name_selection = 0..app.state.generator_name.len();
                app.state.generator_name_anchor = Some(0);
                app.state.generator_name_cursor = app.state.generator_name.len();
                cx.notify();
            } else if app.state.search_focus.is_focused(window) {
                app.state.selection = 0..app.state.query.len();
                cx.notify();
            }
        }))
        .on_action(cx.listener(|app, _: &SwitchFocus, window, cx| {
            if app.state.search_focus.is_focused(window) {
                app.state.focus.focus(window);
            } else {
                app.state.search_focus.focus(window);
            }
            cx.notify();
        }))
        .on_action(cx.listener(|_, _: &Quit, _, cx| cx.quit()))
        .on_action(cx.listener(|_, _: &ToggleFullscreen, window, _| {
            window.toggle_fullscreen();
        }))
}
