//! The container detail pane: logs, stats, and inspect.
//!
//! Streams are cancelled by dropping the producer future, so closing the pane
//! or switching containers tears the stream down — there is no separate abort
//! registry to fall out of sync with what is on screen.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use gpui::prelude::*;
use gpui::{div, Context, Window};
use guise::prelude::*;

use crate::bridge;
use crate::format;
use crate::state::AppState;
use crate::theme;
use model::{Container, ContainerStats, LogOptions, StreamKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Logs,
    Stats,
    Files,
    Terminal,
    Inspect,
}

impl Tab {
    pub fn label(&self) -> &'static str {
        match self {
            Tab::Logs => "Logs",
            Tab::Stats => "Stats",
            Tab::Files => "Files",
            Tab::Terminal => "Terminal",
            Tab::Inspect => "Inspect",
        }
    }

    /// Dev-only initial tab, for screenshotting each detail tab.
    pub fn from_env() -> Tab {
        match std::env::var("HOPPER_TAB").ok().as_deref() {
            Some(t) if t.eq_ignore_ascii_case("stats") => Tab::Stats,
            Some(t) if t.eq_ignore_ascii_case("files") => Tab::Files,
            Some(t) if t.eq_ignore_ascii_case("terminal") => Tab::Terminal,
            Some(t) if t.eq_ignore_ascii_case("inspect") => Tab::Inspect,
            _ => Tab::Logs,
        }
    }

    pub fn all() -> [Tab; 5] {
        [
            Tab::Logs,
            Tab::Stats,
            Tab::Files,
            Tab::Terminal,
            Tab::Inspect,
        ]
    }

    /// Whether the active engine can serve this tab.
    ///
    /// Apple's runtime has no stats, no filesystem browsing and no exec, so
    /// those tabs are not drawn rather than drawn and broken.
    pub fn supported(&self, caps: &model::EngineCapabilities) -> bool {
        match self {
            Tab::Logs | Tab::Inspect => true,
            Tab::Stats => caps.stats,
            Tab::Files => caps.files,
            Tab::Terminal => caps.exec,
        }
    }

    /// The tabs this engine can actually serve.
    pub fn available(caps: &model::EngineCapabilities) -> Vec<Tab> {
        Self::all()
            .into_iter()
            .filter(|t| t.supported(caps))
            .collect()
    }
}

pub struct Detail {
    state: AppState,
    container: Container,
    tab: Tab,
    logs: super::logs::Buffer,
    stream: Option<gpui::Task<()>>,
    request: Option<gpui::Task<()>>,
    stats: Option<ContainerStats>,
    inspect: Option<String>,
    files: Option<gpui::Entity<super::files::Files>>,
    terminal: Option<gpui::Entity<super::terminal::Terminal>>,
    /// Bumped when the container or tab changes; a stream whose generation is
    /// stale stops appending, so output from a previous container can never
    /// leak into the current view.
    generation: u64,
}

impl Detail {
    pub fn new(container: Container, cx: &mut Context<Self>) -> Self {
        let state = AppState::get(cx);
        let mut view = Self {
            state,
            container,
            tab: Tab::from_env(),
            logs: super::logs::Buffer::default(),
            stream: None,
            request: None,
            stats: None,
            inspect: None,
            files: None,
            terminal: None,
            generation: 0,
        };
        view.follow(cx);
        view
    }

    /// Switch to a different container, tearing down whatever was streaming.
    pub fn show(&mut self, container: Container, cx: &mut Context<Self>) {
        if container.id == self.container.id {
            return;
        }
        if let Some(files) = &self.files {
            files.update(cx, |f, cx| f.show(container.id.clone(), cx));
        }
        if let Some(term) = &self.terminal {
            term.update(cx, |t, cx| t.show(container.id.clone(), cx));
        }
        self.container = container;
        self.logs.clear();
        self.stats = None;
        self.inspect = None;
        self.generation += 1;
        self.follow(cx);
        cx.notify();
    }

    pub fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        if self.tab == tab {
            return;
        }
        self.tab = tab;
        self.generation += 1;
        self.follow(cx);
        cx.notify();
    }

    fn append(&mut self, stream: StreamKind, text: String) {
        self.logs.push(stream, text);
    }

    /// Start whatever the active tab needs.
    fn follow(&mut self, cx: &mut Context<Self>) {
        self.stream = None;
        self.request = None;
        if self.tab != Tab::Terminal {
            self.terminal = None;
        }
        let generation = self.generation;
        let id = self.container.id.clone();
        let host = Arc::clone(&self.state.host);
        let entity = cx.entity().downgrade();

        match self.tab {
            // Files and Terminal drive their own lifecycles through entities.
            Tab::Files | Tab::Terminal => {}
            Tab::Logs => {
                let pending = Arc::new(Mutex::new(super::logs::Buffer::default()));
                let notice = Arc::new(Mutex::new(None::<String>));
                let notified = Arc::new(AtomicBool::new(false));
                let update_pending = Arc::clone(&pending);
                let finish_pending = Arc::clone(&pending);
                let update_flag = Arc::clone(&notified);
                let finish_notice = Arc::clone(&notice);
                let finish_entity = entity.clone();
                self.stream = Some(bridge::stream_owned(
                    cx,
                    move |mut tx| async move {
                        let opts = LogOptions {
                            tail: 500,
                            follow: true,
                            ..Default::default()
                        };
                        let callback_pending = Arc::clone(&pending);
                        let signal = Arc::new(tokio::sync::Notify::new());
                        let callback_signal = Arc::clone(&signal);
                        let request = host.stream_logs("detail", &id, &opts, move |line| {
                            // Keep only the bounded newest tail when the UI is
                            // busy. Full notification queues never stop logs.
                            callback_pending
                                .lock()
                                .unwrap()
                                .push(line.stream, line.text);
                            callback_signal.notify_one();
                            true
                        });
                        tokio::pin!(request);
                        let mut tick = tokio::time::interval(std::time::Duration::from_millis(100));
                        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                        let mut dirty = false;
                        loop {
                            tokio::select! {
                                result = &mut request => {
                                    *notice.lock().unwrap() = Some(match result {
                                        Ok(()) => "Log stream ended. Reopen Logs to reconnect.".into(),
                                        Err(error) => format!("Log stream failed: {}. Reopen Logs to reconnect.", error.message),
                                    });
                                    break;
                                },
                                _ = signal.notified(), if !dirty => dirty = true,
                                _ = tick.tick(), if dirty => {
                                    dirty = false;
                                    if !pending.lock().unwrap().lines.is_empty() && !notified.swap(true, Ordering::AcqRel) {
                                        if let Err(error) = tx.try_send(()) {
                                            notified.store(false, Ordering::Release);
                                            if error.is_disconnected() { return; }
                                        }
                                    }
                                }
                            }
                        }
                    },
                    move |(), cx| {
                        update_flag.store(false, Ordering::Release);
                        let batch = std::mem::take(&mut *update_pending.lock().unwrap());
                        let _ = entity.update(cx, |this: &mut Self, cx| {
                            if this.generation == generation {
                                for (stream, text) in batch.lines {
                                    this.append(stream, text);
                                }
                                cx.notify();
                            }
                        });
                    },
                    move |cx| {
                        let batch = std::mem::take(&mut *finish_pending.lock().unwrap());
                        let notice = finish_notice.lock().unwrap().take();
                        let _ = finish_entity.update(cx, |this: &mut Self, cx| {
                            if this.generation == generation {
                                for (stream, text) in batch.lines {
                                    this.append(stream, text);
                                }
                                if let Some(notice) = notice {
                                    this.append(StreamKind::Stderr, format!("[Hopper] {notice}"));
                                }
                                cx.notify();
                            }
                        });
                    },
                ));
            }
            Tab::Stats => {
                self.stream = Some(bridge::stream_owned(
                    cx,
                    move |mut tx| async move {
                        let _ = host
                            .stream_stats(&id, |sample| match tx.try_send(sample) {
                                Ok(()) => true,
                                Err(error) => !error.is_disconnected(),
                            })
                            .await;
                    },
                    {
                        move |sample, cx| {
                            let _ = entity.update(cx, |this: &mut Self, cx| {
                                if this.generation == generation {
                                    this.stats = Some(sample);
                                    cx.notify();
                                }
                            });
                        }
                    },
                    |_| {},
                ));
            }
            Tab::Inspect => {
                self.request = Some(bridge::run_owned(
                    cx,
                    async move { host.container_inspect(&id).await },
                    {
                        move |result, cx| {
                            let _ = entity.update(cx, |this: &mut Self, cx| {
                                if this.generation != generation {
                                    return;
                                }
                                this.inspect = Some(match result {
                                    Ok(value) => serde_json::to_string_pretty(&value)
                                        .unwrap_or_else(|e| e.to_string()),
                                    Err(e) => e.message,
                                });
                                cx.notify();
                            });
                        }
                    },
                ));
            }
        }
    }

    fn logs_body(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        if self.logs.lines.is_empty() {
            return crate::views::message("Waiting for output…");
        }
        gpui::uniform_list(
            "detail-logs",
            self.logs.lines.len(),
            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                let palette = theme::palette(cx);
                range
                    .filter_map(|index| this.logs.lines.get(index))
                    .map(|(stream, text)| {
                        div()
                            .h(gpui::px(18.0))
                            .text_xs()
                            .font_family(theme::MONO_FAMILY)
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_color(if *stream == StreamKind::Stderr {
                                gpui::Hsla::from(gpui::rgb(0xf87171))
                            } else {
                                palette.text_muted
                            })
                            .child(text.clone())
                    })
                    .collect()
            }),
        )
        .size_full()
        .into_any_element()
    }

    fn stats_body(&self, cx: &gpui::App) -> gpui::AnyElement {
        let Some(s) = self.stats.as_ref() else {
            return crate::views::message("Sampling…");
        };
        let palette = theme::palette(cx);
        let tile = |label: &str, value: String| {
            div()
                .flex_1()
                .p_3()
                .rounded_md()
                .bg(palette.bg_subtle)
                .child(
                    Stack::new()
                        .gap(Size::Xs)
                        .child(Text::new(value).size(Size::Lg).bold())
                        .child(Text::new(label.to_string()).size(Size::Xs).dimmed()),
                )
        };
        div()
            .p_4()
            .child(
                Stack::new()
                    .gap(Size::Sm)
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .child(tile("CPU", format!("{:.1}%", s.cpu_percent)))
                            .child(tile(
                                "Memory",
                                format!(
                                    "{} / {}",
                                    format::bytes(s.mem_usage as i64),
                                    format::bytes(s.mem_limit as i64)
                                ),
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .child(tile(
                                "Network",
                                format!(
                                    "↓{} ↑{}",
                                    format::bytes(s.net_rx as i64),
                                    format::bytes(s.net_tx as i64)
                                ),
                            ))
                            .child(tile(
                                "Block I/O",
                                format!(
                                    "r{} w{}",
                                    format::bytes(s.block_read as i64),
                                    format::bytes(s.block_write as i64)
                                ),
                            ))
                            .child(tile("PIDs", s.pids.to_string())),
                    ),
            )
            .into_any_element()
    }
}

impl Render for Detail {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::palette(cx);
        let active = self.tab;

        let mut tabs = Group::new().gap(Size::Xs);
        for tab in Tab::available(&self.state.host.capabilities()) {
            tabs = tabs.child(
                Button::new(tab.label(), tab.label())
                    .size(Size::Xs)
                    .variant(if tab == active {
                        Variant::Light
                    } else {
                        Variant::Subtle
                    })
                    .color(if tab == active {
                        ColorName::Blue
                    } else {
                        ColorName::Gray
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.set_tab(tab, cx))),
            );
        }

        let body = match self.tab {
            Tab::Logs => self.logs_body(cx),
            Tab::Stats => self.stats_body(cx),
            Tab::Files => {
                let id = self.container.id.clone();
                let files = self
                    .files
                    .get_or_insert_with(|| cx.new(|cx| super::files::Files::new(id, cx)))
                    .clone();
                div().size_full().child(files).into_any_element()
            }
            Tab::Terminal => {
                let id = self.container.id.clone();
                let term = self
                    .terminal
                    .get_or_insert_with(|| cx.new(|cx| super::terminal::Terminal::new(id, cx)))
                    .clone();
                div().size_full().child(term).into_any_element()
            }
            Tab::Inspect => match self.inspect.clone() {
                Some(text) => div()
                    .p_3()
                    .text_xs()
                    .font_family(theme::MONO_FAMILY)
                    .child(text)
                    .into_any_element(),
                None => crate::views::message("Loading…"),
            },
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .border_l_1()
            .border_color(palette.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .p_3()
                    .border_b_1()
                    .border_color(palette.border_subtle)
                    .child(Text::new(self.container.name.clone()).size(Size::Sm).bold())
                    .child(tabs),
            )
            .child(div().flex_1().overflow_hidden().child(body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tab_has_a_label() {
        for tab in Tab::all() {
            assert!(!tab.label().is_empty());
        }
        assert_eq!(Tab::Logs.label(), "Logs");
    }

    #[test]
    fn an_engine_api_engine_serves_every_tab() {
        let caps = model::EngineCapabilities::engine_api();
        assert_eq!(Tab::available(&caps).len(), Tab::all().len());
    }

    #[test]
    fn apple_keeps_logs_and_inspect_and_drops_the_rest() {
        // Stats, Files and Terminal have no Apple implementation, so drawing
        // them would offer three tabs that cannot load.
        let caps = model::EngineCapabilities::apple();
        assert_eq!(Tab::available(&caps), vec![Tab::Logs, Tab::Inspect]);
    }
}
