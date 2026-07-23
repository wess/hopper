//! The container detail pane: logs, stats, and inspect.
//!
//! Streams are cancelled by dropping the producer future, so closing the pane
//! or switching containers tears the stream down — there is no separate abort
//! registry to fall out of sync with what is on screen.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{div, Context, Window};
use guise::prelude::*;

use crate::bridge;
use crate::format;
use crate::state::AppState;
use crate::theme;
use model::{Container, ContainerStats, LogOptions, StreamKind};

/// How many log lines to keep. A chatty container emits megabytes a minute;
/// keeping everything would grow without bound and eventually stall the
/// renderer, so the buffer is a window onto the tail.
const MAX_LINES: usize = 2_000;

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
        [Tab::Logs, Tab::Stats, Tab::Files, Tab::Terminal, Tab::Inspect]
    }
}

pub struct Detail {
    state: AppState,
    container: Container,
    tab: Tab,
    lines: Vec<(StreamKind, String)>,
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
            lines: Vec::new(),
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
        self.lines.clear();
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
        self.lines.push((stream, text));
        if self.lines.len() > MAX_LINES {
            // Drop from the front so the newest output stays visible.
            let excess = self.lines.len() - MAX_LINES;
            self.lines.drain(..excess);
        }
    }

    /// Start whatever the active tab needs.
    fn follow(&mut self, cx: &mut Context<Self>) {
        let generation = self.generation;
        let id = self.container.id.clone();
        let host = Arc::clone(&self.state.host);
        let entity = cx.entity().downgrade();

        match self.tab {
            // Files and Terminal drive their own lifecycles through entities.
            Tab::Files | Tab::Terminal => {}
            Tab::Logs => {
                bridge::stream(
                    cx,
                    move |tx| async move {
                        let opts = LogOptions {
                            tail: 500,
                            follow: true,
                            ..Default::default()
                        };
                        let _ = host
                            .stream_logs("detail", &id, &opts, |line| {
                                tx.unbounded_send((line.stream, line.text)).is_ok()
                            })
                            .await;
                    },
                    {
                        move |(stream, text), cx| {
                            let _ = entity.update(cx, |this: &mut Self, cx| {
                                if this.generation == generation {
                                    this.append(stream, text);
                                    cx.notify();
                                }
                            });
                        }
                    },
                    |_| {},
                );
            }
            Tab::Stats => {
                bridge::stream(
                    cx,
                    move |tx| async move {
                        let _ = host
                            .stream_stats(&id, |sample| tx.unbounded_send(sample).is_ok())
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
                );
            }
            Tab::Inspect => {
                bridge::run(
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
                );
            }
        }
    }

    fn logs_body(&self, cx: &gpui::App) -> gpui::AnyElement {
        if self.lines.is_empty() {
            return crate::views::message("Waiting for output…");
        }
        let palette = theme::palette(cx);
        let mut body = div().flex().flex_col().font_family(theme::MONO_FAMILY);
        for (stream, text) in &self.lines {
            body = body.child(
                div()
                    .text_xs()
                    // stderr is coloured so a failing container is obvious
                    // without reading every line.
                    .when(*stream == StreamKind::Stderr, |d| {
                        d.text_color(gpui::Hsla::from(gpui::rgb(0xf87171)))
                    })
                    .when(*stream == StreamKind::Stdout, |d| {
                        d.text_color(palette.text_muted)
                    })
                    .child(text.clone()),
            );
        }
        body.into_any_element()
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
        for tab in Tab::all() {
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
    fn the_log_buffer_is_bounded_to_the_tail() {
        // A chatty container must not grow the buffer without bound.
        let mut lines: Vec<(StreamKind, String)> = Vec::new();
        for i in 0..(MAX_LINES + 500) {
            lines.push((StreamKind::Stdout, format!("line {i}")));
            if lines.len() > MAX_LINES {
                let excess = lines.len() - MAX_LINES;
                lines.drain(..excess);
            }
        }
        assert_eq!(lines.len(), MAX_LINES);
        // The newest output survives; the oldest is what gets dropped.
        assert_eq!(lines.last().unwrap().1, format!("line {}", MAX_LINES + 499));
        assert_eq!(lines.first().unwrap().1, format!("line {}", 500));
    }
}
