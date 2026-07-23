//! The Run dialog — create and start a container from an image.
//!
//! This is the missing link between "I pulled an image" and "I have something
//! running": both the Images list and the Registry (after a pull) open it, and
//! a successful run drops you on the Containers tab so the result is visible,
//! not hidden.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{div, Context, Entity, Window};
use guise::prelude::*;
use model::{PortMapping, RunInput};

use crate::bridge;
use crate::state::{AppState, Route};

/// Parse a "host:container[/proto]" port list into mappings. Forgiving —
/// blanks and half-written entries are skipped rather than rejected, so a
/// stray comma never blocks a run.
fn parse_ports(raw: &str) -> Vec<PortMapping> {
    raw.split(',')
        .filter_map(|part| {
            let part = part.trim();
            if part.is_empty() {
                return None;
            }
            let (mapping, proto) = match part.split_once('/') {
                Some((m, p)) => (m.trim(), Some(p.trim().to_lowercase())),
                None => (part, None),
            };
            let (host, container) = mapping.split_once(':')?;
            let (host, container) = (host.trim(), container.trim());
            if host.is_empty() || container.is_empty() {
                return None;
            }
            Some(PortMapping {
                host: host.to_string(),
                container: container.to_string(),
                proto,
            })
        })
        .collect()
}

pub struct RunDialog {
    state: AppState,
    name: Entity<TextInput>,
    ports: Entity<TextInput>,
    busy: bool,
}

impl RunDialog {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = AppState::get(cx);
        watch(cx, &state.run_target);
        let name = cx.new(|cx| {
            TextInput::new(cx)
                .label("Name")
                .placeholder("optional — Docker names it for you")
        });
        let ports = cx.new(|cx| {
            TextInput::new(cx)
                .label("Publish ports")
                .placeholder("e.g. 8080:80, 5432:5432")
        });
        Self {
            state,
            name,
            ports,
            busy: false,
        }
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.busy = false;
        self.state.run_target.set(cx, None);
        self.name.update(cx, |i, cx| i.set_text("", cx));
        self.ports.update(cx, |i, cx| i.set_text("", cx));
    }

    fn run(&mut self, image: String, cx: &mut Context<Self>) {
        let name = self.name.read(cx).text().trim().to_string();
        let ports = parse_ports(&self.ports.read(cx).text());
        let input = RunInput {
            image: image.clone(),
            name: (!name.is_empty()).then_some(name),
            ports,
            ..Default::default()
        };

        self.busy = true;
        cx.notify();

        let host = Arc::clone(&self.state.host);
        let state = self.state.clone();
        let this = cx.entity().downgrade();
        bridge::run(
            cx,
            async move { host.container_run(&input).await.map_err(|e| e.message) },
            move |result, cx| match result {
                Ok(_) => {
                    state.toast_titled(cx, "Container started", image.clone(), ColorName::Green);
                    // Land on Containers so the running container is visible.
                    state.route.set(cx, Route::Containers);
                    state.bump(cx);
                    if let Some(this) = this.upgrade() {
                        this.update(cx, |this, cx| this.close(cx));
                    }
                }
                Err(e) => {
                    state.toast_titled(cx, "Could not start container", e, ColorName::Red);
                    if let Some(this) = this.upgrade() {
                        this.update(cx, |this, cx| {
                            this.busy = false;
                            cx.notify();
                        });
                    }
                }
            },
        );
    }
}

impl Render for RunDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Closed: render nothing.
        let Some(image) = self.state.run_target.get(cx) else {
            return div();
        };

        let run_image = image.clone();
        let body = Stack::new()
            .gap(Size::Sm)
            .child(
                Text::new(format!("Image  {image}"))
                    .size(Size::Sm)
                    .dimmed(),
            )
            .child(self.name.clone())
            .child(self.ports.clone())
            .child(
                Text::new("Runs detached. Manage it from the Containers tab.")
                    .size(Size::Xs)
                    .dimmed(),
            )
            .child(
                Group::new()
                    .justify(Justify::End)
                    .gap(Size::Sm)
                    .child(
                        Button::new("run-cancel", "Cancel")
                            .variant(Variant::Default)
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                    )
                    .child(
                        Button::new("run-go", if self.busy { "Starting…" } else { "Run" })
                            .color(ColorName::Green)
                            .disabled(self.busy)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.run(run_image.clone(), cx)
                            })),
                    ),
            );

        div().child(
            Modal::new()
                .title("Run a container")
                .width(440.0)
                .on_close(cx.listener(|this, _, _, cx| this.close(cx)))
                .child(body),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ports_parse_host_and_container() {
        let p = parse_ports("8080:80");
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].host, "8080");
        assert_eq!(p[0].container, "80");
        assert_eq!(p[0].proto, None);
    }

    #[test]
    fn ports_take_a_protocol_and_a_list() {
        let p = parse_ports("53:53/udp, 8080:80");
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].proto.as_deref(), Some("udp"));
        assert_eq!(p[1].host, "8080");
    }

    #[test]
    fn ports_skip_blanks_and_half_written_entries() {
        // A trailing comma or a lone port must not block the run.
        let p = parse_ports("8080:80, , 9090");
        assert_eq!(p.len(), 1);
        assert!(parse_ports("").is_empty());
    }
}
