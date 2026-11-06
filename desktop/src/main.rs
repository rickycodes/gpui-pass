#![forbid(unsafe_code)]

mod search;
mod theme;

use gpui::{
    App, Application, Bounds, Context, FocusHandle, FontWeight, KeyBinding, ScrollStrategy,
    SharedString, Subscription, TitlebarOptions, UniformListScrollHandle, Window, WindowBounds,
    WindowOptions, actions, div, prelude::*, px, rgb, size, uniform_list,
};
use gpui_pass_core::{
    Entry, Error, Gpg, RevealRequest, Secret, Session, Store, default_store_path, harden_process,
};
use std::{
    ops::Range,
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};
use theme::*;
use zeroize::Zeroizing;

actions!(
    pass,
    [
        Hide,
        Reveal,
        Refresh,
        FocusSearch,
        NextEntry,
        PreviousEntry,
        Backspace,
        Delete,
        Left,
        Right,
        Start,
        End,
        SelectAll,
        SwitchFocus,
        ToggleFullscreen,
        Quit
    ]
);

struct Options {
    store: Option<PathBuf>,
    gpg: PathBuf,
    demo: bool,
}

impl Options {
    fn parse() -> Result<Option<Self>, String> {
        let mut options = Self {
            store: None,
            gpg: PathBuf::from("/usr/bin/gpg"),
            demo: false,
        };
        let mut args = std::env::args_os().skip(1);
        while let Some(arg) = args.next() {
            match arg.to_str() {
                Some("--demo") => options.demo = true,
                Some("--store") => {
                    options.store = Some(args.next().ok_or("--store requires a directory")?.into())
                }
                Some("--gpg") => {
                    options.gpg = args
                        .next()
                        .ok_or("--gpg requires an absolute executable path")?
                        .into()
                }
                Some("--help" | "-h") => {
                    println!(
                        "gpui-pass [--store DIRECTORY] [--gpg /absolute/path/to/gpg] [--demo]\n\nRead-only pass store browser. Defaults to PASSWORD_STORE_DIR or ~/.password-store.\n--demo uses fictional entries in memory and never opens a password store."
                    );
                    return Ok(None);
                }
                _ => return Err("Unknown argument. Use --help for usage.".into()),
            }
        }
        if options.demo && options.store.is_some() {
            return Err("Use --demo or --store, separately.".into());
        }
        Ok(Some(options))
    }
}

type LoadedStore = Result<(Store, Vec<Entry>), Error>;

pub(crate) struct PassApp {
    session: Session,
    entries: Vec<Entry>,
    filtered: Vec<usize>,
    list_scroll: UniformListScrollHandle,
    query: String,
    selection: Range<usize>,
    marked: Option<Range<usize>>,
    input_bounds: Option<Bounds<gpui::Pixels>>,
    focus: FocusHandle,
    search_focus: FocusHandle,
    active: bool,
    store: Option<Store>,
    path: Option<PathBuf>,
    gpg: PathBuf,
    demo: bool,
    load: Option<Receiver<LoadedStore>>,
    reveal: Option<(RevealRequest, Receiver<Result<Secret, Error>>)>,
    message: Option<String>,
    seconds: u64,
    _activation: Subscription,
}

impl PassApp {
    fn new(options: Options, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let search_focus = cx.focus_handle();
        let subscription = cx.observe_window_activation(window, |app, window, cx| {
            app.active = window.is_window_active();
            if !app.active {
                app.session.deactivate();
            }
            cx.notify();
        });
        search_focus.focus(window);
        let mut app = Self {
            session: Session::default(),
            entries: Vec::new(),
            filtered: Vec::new(),
            list_scroll: UniformListScrollHandle::new(),
            query: String::new(),
            selection: 0..0,
            marked: None,
            input_bounds: None,
            focus,
            search_focus,
            active: window.is_window_active(),
            store: None,
            path: options.store.or_else(|| default_store_path().ok()),
            gpg: options.gpg,
            demo: options.demo,
            load: None,
            reveal: None,
            message: None,
            seconds: 0,
            _activation: subscription,
        };
        app.refresh(cx);
        cx.spawn(async move |entity, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                if entity.update(cx, |app, cx| app.tick(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        app
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.load.is_some() {
            return;
        }
        self.session.select(None);
        self.reveal = None;
        self.message = None;
        self.store = None;
        self.entries.clear();
        self.filtered.clear();
        if self.demo {
            self.entries = [
                "Development/github.com.gpg",
                "Development/linear.app.gpg",
                "Email/proton.me.gpg",
                "Personal/airbnb.com.gpg",
                "Personal/figma.com.gpg",
                "Personal/notion.so.gpg",
                "Servers/atlas.gpg",
                "Servers/backup.gpg",
            ]
            .into_iter()
            .map(|path| Entry::from_relative_file(path).expect("static demo path"))
            .collect();
            self.filter();
        } else if let Some(path) = self.path.clone() {
            let (sender, receiver) = mpsc::sync_channel(1);
            self.load = Some(receiver);
            std::thread::spawn(move || {
                let result = Store::open(path)
                    .and_then(|store| store.entries().map(|entries| (store, entries)));
                let _ = sender.send(result);
            });
        } else {
            self.message = Some(Error::MissingHome.to_string());
        }
        cx.notify();
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        if let Some(result) = self
            .load
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok())
        {
            self.load = None;
            match result {
                Ok((store, entries)) => {
                    self.store = Some(store);
                    self.entries = entries;
                    self.filter();
                }
                Err(error) => self.message = Some(error.to_string()),
            }
            cx.notify();
        }
        if let Some(result) = self
            .reveal
            .as_ref()
            .and_then(|(_, receiver)| receiver.try_recv().ok())
        {
            let (request, _) = self.reveal.take().expect("request available");
            match result {
                Ok(secret) => {
                    if !self
                        .session
                        .finish_reveal(&request, secret, self.active, Instant::now())
                    {
                        self.message = Some(
                            "Entry stayed hidden. Return to this window and reveal again.".into(),
                        );
                    }
                }
                Err(error) => {
                    if self.session.fail_reveal(&request) {
                        self.message = Some(error.to_string());
                    }
                }
            }
            cx.notify();
        }
        if self.session.expire(Instant::now()) {
            cx.notify();
        }
        let seconds = self.session.remaining_seconds(Instant::now());
        if self.seconds != seconds {
            self.seconds = seconds;
            cx.notify();
        }
    }

    fn filter(&mut self) {
        self.session.hide();
        self.reveal = None;
        self.message = None;
        self.filtered = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, entry)| entry.matches(&self.query).then_some(i))
            .collect();
        let first = self
            .filtered
            .first()
            .map(|index| self.entries[*index].clone());
        self.session.select(first);
        self.list_scroll.scroll_to_item(0, ScrollStrategy::Top);
    }

    fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        self.session.select(self.entries.get(index).cloned());
        self.reveal = None;
        self.message = None;
        cx.notify();
    }

    fn navigate(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.filtered.is_empty() {
            return;
        }
        let position = self
            .filtered
            .iter()
            .position(|index| self.session.selected() == self.entries.get(*index))
            .unwrap_or(0);
        let next = (position as isize + delta).clamp(0, self.filtered.len() as isize - 1) as usize;
        self.choose(self.filtered[next], cx);
        self.list_scroll
            .scroll_to_item(next, ScrollStrategy::Center);
    }

    fn hide(&mut self, cx: &mut Context<Self>) {
        self.session.hide();
        self.reveal = None;
        self.message = None;
        cx.notify();
    }

    fn reveal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window);
        if self.session.is_pending() || !window.is_window_active() {
            return;
        }
        if self.session.revealed(Instant::now()).is_some() {
            self.hide(cx);
            return;
        }
        let Some(request) = self.session.begin_reveal() else {
            return;
        };
        self.message = None;
        let result = if self.demo {
            let secret = Secret::from_bytes(Zeroizing::new(b"demo-only-not-a-real-password\nusername: alex@example.test\n\nA fictional entry for exploring the interface.\n".to_vec()));
            let (sender, receiver) = mpsc::sync_channel(1);
            let _ = sender.send(secret);
            receiver
        } else {
            let gpg = match Gpg::new(&self.gpg) {
                Ok(gpg) => gpg,
                Err(error) => {
                    self.session.fail_reveal(&request);
                    self.message = Some(error.to_string());
                    cx.notify();
                    return;
                }
            };
            let Some(store) = self.store.clone() else {
                self.session.fail_reveal(&request);
                return;
            };
            let entry = request.entry.clone();
            let cancel = request.cancellation.clone();
            let (sender, receiver) = mpsc::sync_channel(1);
            std::thread::spawn(move || {
                let _ = sender.send(gpg.decrypt(&store, &entry, &cancel));
            });
            receiver
        };
        self.reveal = Some((request, result));
        cx.notify();
    }

    fn open_store(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.hide(cx);
        let picker = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open password store".into()),
        });
        cx.spawn_in(window, async move |entity, cx| {
            if let Ok(Ok(Some(paths))) = picker.await {
                if let Some(path) = paths.into_iter().next() {
                    let _ = entity.update(cx, |app, cx| {
                        app.load = None;
                        app.demo = false;
                        app.path = Some(path);
                        app.refresh(cx);
                    });
                }
            }
        })
        .detach();
    }

    fn entry_row(&self, index: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let entry = &self.entries[index];
        let active = self.session.selected() == Some(entry);
        div()
            .id(("entry", index))
            .w_full()
            .flex()
            .flex_col()
            .gap(px(5.))
            .px(px(22.))
            .py(px(16.))
            .h(px(76.))
            .overflow_hidden()
            .border_b_1()
            .border_color(rgb(LINE))
            .border_l_2()
            .border_color(rgb(if active { GREEN } else { LINE }))
            .bg(rgb(if active { TINT } else { PAPER }))
            .cursor_pointer()
            .hover(|style| style.bg(rgb(0xebeee6)))
            .on_click(cx.listener(move |app, _, window, cx| {
                app.focus.focus(window);
                app.choose(index, cx);
            }))
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(14.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(entry.title().to_owned()),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .truncate()
                    .text_size(px(11.))
                    .text_color(rgb(MUTED))
                    .child(if entry.folder().is_empty() {
                        "Root".to_owned()
                    } else {
                        entry.folder().to_owned()
                    }),
            )
            .into_any_element()
    }

    fn sidebar(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        div()
            .w(px(302.))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(rgb(LINE))
            .bg(rgb(PAPER))
            .child(
                div()
                    .px(px(22.))
                    .pt(px(26.))
                    .pb(px(18.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(label("YOUR ENTRIES"))
                    .child(
                        div()
                            .font_family(MONO)
                            .text_size(px(11.))
                            .text_color(rgb(MUTED))
                            .child(format!("{:02}", self.filtered.len())),
                    ),
            )
            .child(
                div()
                    .px(px(18.))
                    .pb(px(20.))
                    .child(self.search_field(window, cx)),
            )
            .child(
                div()
                    .id("entry-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .when(!self.filtered.is_empty(), |view| {
                        view.child(
                            uniform_list(
                                "entries",
                                self.filtered.len(),
                                cx.processor(|app, range: Range<usize>, _, cx| {
                                    range
                                        .map(|position| app.entry_row(app.filtered[position], cx))
                                        .collect::<Vec<_>>()
                                }),
                            )
                            .track_scroll(self.list_scroll.clone())
                            .size_full(),
                        )
                    })
                    .when(self.filtered.is_empty(), |view| {
                        view.child(
                            div()
                                .p(px(24.))
                                .text_size(px(13.))
                                .text_color(rgb(MUTED))
                                .child(if self.load.is_some() {
                                    "Reading entry names…"
                                } else if self.query.is_empty() {
                                    "No entries to show."
                                } else {
                                    "No matching entries.\nTry another search."
                                }),
                        )
                    }),
            )
            .child(
                div()
                    .p(px(22.))
                    .border_t_1()
                    .border_color(rgb(LINE))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(label(if self.demo {
                        "DEMO STORE"
                    } else {
                        "CURRENT STORE"
                    }))
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_family(MONO)
                            .text_color(rgb(MUTED))
                            .overflow_hidden()
                            .child(self.path_label()),
                    )
                    .child(
                        div()
                            .id("open-store")
                            .mt(px(8.))
                            .text_size(px(12.))
                            .text_color(rgb(GREEN))
                            .cursor_pointer()
                            .on_click(cx.listener(|app, _, window, cx| app.open_store(window, cx)))
                            .child("Open another store  ↗"),
                    ),
            )
            .into_any_element()
    }

    fn path_label(&self) -> String {
        if self.demo {
            return "Fictional entries · in memory".into();
        }
        self.path.as_ref().map_or_else(
            || "No store selected".into(),
            |path| {
                path.to_string_lossy()
                    .chars()
                    .filter(|c| !c.is_control())
                    .collect()
            },
        )
    }

    fn details(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let entry = self.session.selected();
        let now = Instant::now();
        let revealed = self.session.revealed(now);
        let pending = self.session.is_pending();
        let mut detail = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(WHITE));
        if let Some(entry) = entry {
            let folder = if entry.folder().is_empty() {
                "Root"
            } else {
                entry.folder()
            };
            detail = detail
                .child(
                    div()
                        .px(px(46.))
                        .pt(px(40.))
                        .pb(px(28.))
                        .flex()
                        .flex_col()
                        .gap(px(17.))
                        .child(label(format!("STORE   /   {}", folder.to_uppercase())))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(16.))
                                .child(
                                    div()
                                        .text_size(px(36.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(entry.title().to_owned()),
                                )
                                .child(
                                    div()
                                        .border_1()
                                        .border_color(rgb(LINE))
                                        .px(px(10.))
                                        .py(px(6.))
                                        .font_family(MONO)
                                        .text_size(px(10.))
                                        .text_color(rgb(MUTED))
                                        .child(".gpg"),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(rgb(MUTED))
                                .child(entry.name().to_owned()),
                        ),
                )
                .child(
                    div()
                        .px(px(46.))
                        .pb(px(24.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .border_b_1()
                        .border_color(rgb(LINE))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(7.))
                                .child(div().size(px(5.)).bg(rgb(GREEN)))
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(rgb(MUTED))
                                        .child("Encrypted on disk"),
                                ),
                        )
                        .child(label("READ ONLY")),
                );

            let mut body = div()
                .id("detail-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p(px(46.))
                .flex()
                .flex_col()
                .gap(px(24.));
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(label("PASSWORD"))
                    .child(
                        div()
                            .font_family(MONO)
                            .text_size(px(10.))
                            .text_color(rgb(if revealed.is_some() { GREEN } else { MUTED }))
                            .child(if revealed.is_some() {
                                format!("HIDES IN {:02}s", self.seconds)
                            } else {
                                "HIDDEN".into()
                            }),
                    ),
            );
            let mut password = div()
                .border_1()
                .border_color(rgb(LINE))
                .bg(rgb(PAPER))
                .p(px(22.))
                .min_h(px(84.));
            if let Some(secret) = revealed {
                let text = secret.expose();
                let first_line = text.split('\n').next().unwrap_or("").trim_end_matches('\r');
                password = password.child(
                    div()
                        .id("password-text")
                        .overflow_x_scroll()
                        .font_family(MONO)
                        .text_size(px(18.))
                        .whitespace_nowrap()
                        .child(first_line.chars().take(4096).collect::<String>()),
                );
            } else {
                password = password.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(24.))
                                .text_color(rgb(0x8c968b))
                                .child("••••••••••••"),
                        )
                        .child(
                            div()
                                .font_family(MONO)
                                .text_size(px(10.))
                                .text_color(rgb(MUTED))
                                .child("CONCEALED"),
                        ),
                );
            }
            body = body.child(password).child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(18.))
                    .child(
                        button(
                            "reveal",
                            if pending {
                                "Cancel reveal"
                            } else if revealed.is_some() {
                                "Hide password"
                            } else {
                                "Reveal password  ↗"
                            },
                            true,
                        )
                        .on_click(cx.listener(|app, _, window, cx| {
                            if app.session.is_pending() {
                                app.hide(cx);
                            } else {
                                app.reveal(window, cx);
                            }
                        })),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(MUTED))
                            .child(if pending {
                                "Check PIN prompt / touch your YubiKey"
                            } else {
                                "Enter to reveal · Esc to hide"
                            }),
                    ),
            );

            if let Some(secret) = revealed {
                if let Some((_, notes)) = secret
                    .expose()
                    .split_once('\n')
                    .filter(|(_, notes)| !notes.trim().is_empty())
                {
                    body = body.child(
                        div()
                            .mt(px(12.))
                            .pt(px(24.))
                            .border_t_1()
                            .border_color(rgb(LINE))
                            .flex()
                            .flex_col()
                            .gap(px(18.))
                            .child(label("ENTRY CONTENT"))
                            .child(
                                div()
                                    .font_family(MONO)
                                    .text_size(px(12.))
                                    .line_height(px(23.))
                                    .child(notes.chars().take(16_384).collect::<String>()),
                            ),
                    );
                }
                if secret.expose().len() > 16_384 {
                    body = body.child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(MUTED))
                            .child("Long content is truncated in this preview."),
                    );
                }
            } else {
                body = body.child(div().mt(px(16.)).pt(px(24.)).border_t_1().border_color(rgb(LINE)).flex().flex_col().gap(px(10.))
                    .child(label("ONLY WHEN YOU NEED IT"))
                    .child(div().max_w(px(385.)).text_size(px(13.)).line_height(px(23.)).text_color(rgb(MUTED))
                        .child("Reveal this entry for 30 seconds. It hides when you leave the window or choose another entry.")));
            }
            detail = detail.child(body);
        } else {
            detail = detail.child(div().flex_1().flex().flex_col().justify_center().px(px(60.)).gap(px(18.))
                .child(div().text_size(px(56.)).font_weight(FontWeight::LIGHT).text_color(rgb(GREEN)).child("[ / ]"))
                .child(div().text_size(px(30.)).child(if self.load.is_some() { "Opening your store." } else if !self.query.is_empty() { "Nothing here. Yet." } else { "Your passwords, at home." }))
                .child(div().max_w(px(380.)).text_size(px(14.)).line_height(px(24.)).text_color(rgb(MUTED))
                    .child(if !self.query.is_empty() { "Search by an entry name or folder." } else { "Open an existing pass store to browse your entries. Your keys stay with GnuPG." }))
                .child(button("empty-open-store", "Open password store  ↗", true).w(px(208.)).on_click(cx.listener(|app, _, window, cx| app.open_store(window, cx)))));
        }
        if let Some(message) = &self.message {
            detail = detail.child(
                div()
                    .mx(px(46.))
                    .mb(px(20.))
                    .border_l_2()
                    .border_color(rgb(ERROR))
                    .pl(px(14.))
                    .text_size(px(12.))
                    .line_height(px(20.))
                    .text_color(rgb(ERROR))
                    .child(message.clone()),
            );
        }
        detail
            .child(
                div()
                    .px(px(46.))
                    .py(px(20.))
                    .border_t_1()
                    .border_color(rgb(LINE))
                    .flex()
                    .justify_between()
                    .child(label(if self.demo {
                        "FICTIONAL DATA"
                    } else {
                        "LOCAL STORE"
                    }))
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(rgb(MUTED))
                            .child("Nothing decrypted until you ask."),
                    ),
            )
            .into_any_element()
    }
}

impl Render for PassApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(PAPER))
            .text_color(rgb(INK))
            .font_family("DejaVu Sans")
            .text_size(px(14.))
            .key_context("Pass")
            .track_focus(&self.focus)
            .on_action(cx.listener(|app, _: &Hide, _, cx| app.hide(cx)))
            .on_action(cx.listener(|app, _: &Reveal, window, cx| app.reveal(window, cx)))
            .on_action(cx.listener(|app, _: &Refresh, _, cx| app.refresh(cx)))
            .on_action(cx.listener(|app, _: &FocusSearch, window, cx| {
                app.hide(cx);
                app.search_focus.focus(window);
                app.selection = 0..app.query.len();
                cx.notify();
            }))
            .on_action(cx.listener(|app, _: &NextEntry, _, cx| app.navigate(1, cx)))
            .on_action(cx.listener(|app, _: &PreviousEntry, _, cx| app.navigate(-1, cx)))
            .on_action(cx.listener(|app, _: &Backspace, window, cx| app.backspace(window, cx)))
            .on_action(cx.listener(|app, _: &Delete, window, cx| app.delete(window, cx)))
            .on_action(cx.listener(|app, _: &Left, window, cx| app.move_cursor(-1, window, cx)))
            .on_action(cx.listener(|app, _: &Right, window, cx| app.move_cursor(1, window, cx)))
            .on_action(cx.listener(|app, _: &Start, window, cx| app.move_edge(false, window, cx)))
            .on_action(cx.listener(|app, _: &End, window, cx| app.move_edge(true, window, cx)))
            .on_action(cx.listener(|app, _: &SelectAll, window, cx| {
                if app.search_focus.is_focused(window) {
                    app.selection = 0..app.query.len();
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|app, _: &SwitchFocus, window, cx| {
                if app.search_focus.is_focused(window) {
                    app.focus.focus(window);
                } else {
                    app.search_focus.focus(window);
                }
                cx.notify();
            }))
            .on_action(cx.listener(|_, _: &Quit, _, cx| cx.quit()))
            .on_action(cx.listener(|_, _: &ToggleFullscreen, window, _| {
                window.toggle_fullscreen();
            }))
            .child(
                div()
                    .id("titlebar")
                    .w_full()
                    .h(px(76.))
                    .flex_shrink_0()
                    .px(px(26.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .bg(rgb(INK))
                    .on_mouse_down(gpui::MouseButton::Left, |event, window, cx| {
                        if event.click_count == 2 {
                            window.toggle_fullscreen();
                        } else {
                            window.start_window_move();
                        }
                        cx.stop_propagation();
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(14.))
                            .child(
                                div()
                                    .w(px(32.))
                                    .h(px(32.))
                                    .border_1()
                                    .border_color(rgb(0x809287))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(16.))
                                    .text_color(rgb(0xe0edcb))
                                    .font_family(MONO)
                                    .child("/"),
                            )
                            .child(
                                div()
                                    .text_size(px(22.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(WHITE))
                                    .child("pass"),
                            )
                            .child(
                                div()
                                    .ml(px(18.))
                                    .text_size(px(11.))
                                    .text_color(rgb(0xaebbb0))
                                    .child("A quiet place for your passwords."),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(18.))
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .font_family(MONO)
                                    .text_color(rgb(0xc7d6be))
                                    .child(if self.demo { "DEMO / 01" } else { "LOCAL / 01" }),
                            )
                            .child(
                                div()
                                    .id("refresh")
                                    .text_size(px(12.))
                                    .text_color(rgb(WHITE))
                                    .cursor_pointer()
                                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation();
                                    })
                                    .on_click(cx.listener(|app, _, _, cx| app.refresh(cx)))
                                    .child("↻  Refresh"),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.sidebar(window, cx))
                    .child(self.details(cx)),
            )
            .child(
                div()
                    .h(px(30.))
                    .flex_shrink_0()
                    .px(px(24.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_t_1()
                    .border_color(rgb(LINE))
                    .child(
                        div().text_size(px(10.)).text_color(rgb(MUTED)).child(
                            "Ctrl F  Search     ↑ ↓  Browse     Enter  Reveal     Esc  Hide",
                        ),
                    )
                    .child(
                        div()
                            .font_family(MONO)
                            .text_size(px(9.))
                            .text_color(rgb(MUTED))
                            .child("GPUI PASS  /  PROTOTYPE"),
                    ),
            )
    }
}

fn main() {
    let options = match Options::parse() {
        Ok(Some(options)) => options,
        Ok(None) => return,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    if harden_process().is_err() {
        eprintln!("Could not disable process dumps; refusing to open the password client.");
        std::process::exit(1);
    }
    Application::new().run(move |cx: &mut App| {
        cx.bind_keys([
            KeyBinding::new("escape", Hide, Some("Pass")),
            KeyBinding::new("enter", Reveal, Some("Pass")),
            KeyBinding::new("ctrl-r", Refresh, Some("Pass")),
            KeyBinding::new("ctrl-f", FocusSearch, Some("Pass")),
            KeyBinding::new("down", NextEntry, Some("Pass")),
            KeyBinding::new("up", PreviousEntry, Some("Pass")),
            KeyBinding::new("backspace", Backspace, Some("Pass")),
            KeyBinding::new("delete", Delete, Some("Pass")),
            KeyBinding::new("left", Left, Some("Pass")),
            KeyBinding::new("right", Right, Some("Pass")),
            KeyBinding::new("home", Start, Some("Pass")),
            KeyBinding::new("end", End, Some("Pass")),
            KeyBinding::new("ctrl-a", SelectAll, Some("Pass")),
            KeyBinding::new("tab", SwitchFocus, Some("Pass")),
            KeyBinding::new("shift-tab", SwitchFocus, Some("Pass")),
            KeyBinding::new("ctrl-q", Quit, Some("Pass")),
            KeyBinding::new("f11", ToggleFullscreen, Some("Pass")),
        ]);
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let bounds = Bounds::centered(None, size(px(1120.), px(780.)), cx);
        if cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(940.), px(640.))),
                    titlebar: Some(TitlebarOptions {
                        title: Some(SharedString::from("pass — Password store")),
                        ..Default::default()
                    }),
                    app_id: Some("dev.gpui-pass.desktop".into()),
                    ..Default::default()
                },
                move |window, cx| cx.new(|cx| PassApp::new(options, window, cx)),
            )
            .is_err()
        {
            eprintln!("Could not open a GPUI window. Check your display and Vulkan driver.");
            cx.quit();
        }
        cx.activate(true);
    });
}
