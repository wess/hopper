//! An interactive shell in a container — Docker Desktop's Exec tab.
//!
//! Backed by `exec::start`, which hijacks the socket for a real bidirectional
//! stream. The TypeScript build faked this with a textarea and a line buffer,
//! so anything that redraws — top, vim, a progress bar — rendered as escape
//! sequences. This drives a real session: keystrokes go in, the container's
//! output comes back.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{div, Context, KeyDownEvent, Window};

use crate::bridge;
use crate::state::AppState;
use crate::theme;

/// How many lines of scrollback the screen keeps. A `find /` scrolls forever;
/// the screen is a window onto the tail, matching the log viewer.
const MAX_LINES: usize = 3_000;

pub struct Terminal {
    state: AppState,
    container: String,
    /// The interpreted terminal screen. Control codes (carriage return,
    /// erase, cursor moves, SGR) are consumed rather than shown, so an
    /// interactive shell reads cleanly.
    screen: super::ansi::Screen,
    session: Option<Arc<host::docker_exec::Session>>,
    started: bool,
    error: Option<String>,
}

impl Terminal {
    pub fn new(container: String, cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            state: AppState::get(cx),
            container,
            screen: super::ansi::Screen::new(MAX_LINES),
            session: None,
            started: false,
            error: None,
        };
        view.connect(cx);
        view
    }

    /// Retarget at a different container, ending the current session.
    pub fn show(&mut self, container: String, cx: &mut Context<Self>) {
        if container == self.container {
            return;
        }
        self.container = container;
        // Dropping the session closes its socket, ending the shell.
        self.session = None;
        self.screen = super::ansi::Screen::new(MAX_LINES);
        self.started = false;
        self.connect(cx);
        cx.notify();
    }

    fn append(&mut self, text: &str) {
        self.screen.feed(text.as_bytes());
    }

    fn connect(&mut self, cx: &mut Context<Self>) {
        self.started = true;
        let host = Arc::clone(&self.state.host);
        let id = self.container.clone();
        let entity = cx.entity().downgrade();

        // The exec session pushes output through a channel; the UI drains it
        // on the main thread.
        let (tx, mut rx) = futures::channel::mpsc::unbounded::<String>();

        cx.spawn(async move |_, cx| {
            while let Some(text) = futures::StreamExt::next(&mut rx).await {
                if cx
                    .update(|cx| {
                        let _ = entity.update(cx, |this: &mut Terminal, cx| {
                            this.append(&text);
                            cx.notify();
                        });
                    })
                    .is_err()
                {
                    return;
                }
            }
        })
        .detach();

        let entity2 = cx.entity().downgrade();
        bridge::run(
            cx,
            async move {
                host.exec_start(&id, None, true, move |chunk| tx.unbounded_send(chunk).is_ok())
                    .await
                    .map(Arc::new)
                    .map_err(|e| e.message)
            },
            move |result, cx| {
                let _ = entity2.update(cx, |this: &mut Terminal, cx| {
                    match result {
                        Ok(session) => this.session = Some(session),
                        Err(e) => this.error = Some(e),
                    }
                    cx.notify();
                });
            },
        );
    }

    /// Send a keystroke to the container.
    fn key(&mut self, event: &KeyDownEvent) {
        let Some(session) = &self.session else {
            return;
        };
        let bytes = encode_key(&event.keystroke);
        if !bytes.is_empty() {
            session.write(bytes);
        }
    }
}

/// Translate a gpui keystroke into the bytes a terminal expects.
///
/// Enough of the control set to actually use a shell: Enter, Tab, Backspace,
/// the arrows, and Ctrl-C / Ctrl-D / Ctrl-anything.
pub fn encode_key(k: &gpui::Keystroke) -> Vec<u8> {
    let key = k.key.as_str();
    // Ctrl-<letter> maps to control codes 0x01..0x1a.
    if k.modifiers.control && key.len() == 1 {
        if let Some(c) = key.chars().next().filter(|c| c.is_ascii_alphabetic()) {
            return vec![(c.to_ascii_lowercase() as u8) - b'a' + 1];
        }
    }
    match key {
        "enter" => vec![b'\r'],
        "tab" => vec![b'\t'],
        "backspace" => vec![0x7f],
        "escape" => vec![0x1b],
        "up" => b"\x1b[A".to_vec(),
        "down" => b"\x1b[B".to_vec(),
        "right" => b"\x1b[C".to_vec(),
        "left" => b"\x1b[D".to_vec(),
        "space" => vec![b' '],
        other if other.chars().count() == 1 => {
            // A printable key; honor shift for letters.
            let mut s = other.to_string();
            if k.modifiers.shift {
                s = s.to_uppercase();
            }
            s.into_bytes()
        }
        _ => Vec::new(),
    }
}

impl Render for Terminal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::palette(cx);

        let body = if let Some(err) = self.error.clone() {
            crate::views::failure("Could not start a shell", &err)
        } else if self.session.is_none() {
            crate::views::message("Starting a shell…")
        } else {
            div()
                .size_full()
                .p_2()
                .font_family(theme::MONO_FAMILY)
                .text_xs()
                .text_color(palette.text_muted)
                .child(self.screen.render())
                .into_any_element()
        };

        div()
            .size_full()
            .track_focus(&cx.focus_handle())
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                this.key(event);
                cx.notify();
            }))
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Keystroke, Modifiers};

    fn keystroke(key: &str, mods: Modifiers) -> Keystroke {
        Keystroke {
            modifiers: mods,
            key: key.to_string(),
            key_char: None,
        }
    }

    #[test]
    fn enter_becomes_a_carriage_return() {
        // A shell wants CR, not LF, to execute a line.
        assert_eq!(encode_key(&keystroke("enter", Modifiers::none())), vec![b'\r']);
    }

    #[test]
    fn ctrl_c_sends_the_interrupt_byte() {
        let mut mods = Modifiers::none();
        mods.control = true;
        // 0x03 is ETX — what a terminal sends for Ctrl-C.
        assert_eq!(encode_key(&keystroke("c", mods)), vec![0x03]);
    }

    #[test]
    fn ctrl_d_sends_end_of_transmission() {
        let mut mods = Modifiers::none();
        mods.control = true;
        assert_eq!(encode_key(&keystroke("d", mods)), vec![0x04]);
    }

    #[test]
    fn the_arrows_send_their_escape_sequences() {
        assert_eq!(encode_key(&keystroke("up", Modifiers::none())), b"\x1b[A");
        assert_eq!(encode_key(&keystroke("left", Modifiers::none())), b"\x1b[D");
    }

    #[test]
    fn backspace_sends_delete_not_a_literal() {
        assert_eq!(encode_key(&keystroke("backspace", Modifiers::none())), vec![0x7f]);
    }

    #[test]
    fn a_printable_letter_passes_through() {
        assert_eq!(encode_key(&keystroke("a", Modifiers::none())), b"a");
    }

    #[test]
    fn shift_uppercases_a_letter() {
        let mut mods = Modifiers::none();
        mods.shift = true;
        assert_eq!(encode_key(&keystroke("a", mods)), b"A");
    }

    #[test]
    fn an_unmapped_key_produces_nothing_rather_than_garbage() {
        assert!(encode_key(&keystroke("f1", Modifiers::none())).is_empty());
    }

}
