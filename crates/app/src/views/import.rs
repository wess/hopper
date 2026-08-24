//! Bringing an existing Docker install across.
//!
//! Hopper's whole pitch on macOS is that you stop running Docker Desktop, and
//! that is only true if what you already have comes with you. This view finds
//! the other engine, lists what it holds, and copies the selection over.
//!
//! Nothing is removed from the source. If the import goes wrong, Docker is
//! still exactly where it was.

use std::collections::BTreeSet;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{div, px, Context, Window};
use guise::prelude::*;
use model::{MigrationItem, MigrationKind, MigrationPlan, MigrationProgress, MigrationScan};

use crate::bridge;
use crate::state::AppState;
use crate::theme;

/// Where the view is in the scan → choose → copy sequence.
#[derive(Clone, Debug, PartialEq)]
pub enum Stage {
    /// Nothing asked for yet.
    Idle,
    Scanning,
    /// A scan came back. May still hold nothing.
    Chosen(Box<MigrationScan>),
    Importing,
    Done(String),
}

/// Everything selectable, flattened, so the list renders in one pass.
pub fn rows(scan: &MigrationScan) -> Vec<&MigrationItem> {
    scan.images
        .iter()
        .chain(scan.volumes.iter())
        .chain(scan.networks.iter())
        .chain(scan.containers.iter())
        .collect()
}

/// Everything is selected the first time a scan lands.
///
/// The common case is "bring it all across"; unticking a few is less work than
/// ticking twenty.
pub fn select_all(scan: &MigrationScan) -> BTreeSet<String> {
    rows(scan).into_iter().map(key).collect()
}

/// A selection key that cannot collide between kinds — an image and a volume
/// are allowed to share a name.
pub fn key(item: &MigrationItem) -> String {
    format!("{}:{}", item.kind.as_str(), item.id)
}

/// Turn the ticked rows into a plan, keeping the scan's pinned source.
pub fn plan_from(scan: &MigrationScan, selected: &BTreeSet<String>) -> MigrationPlan {
    let mut plan = MigrationPlan {
        source: scan.source_endpoint.clone(),
        ..Default::default()
    };
    for item in rows(scan) {
        if !selected.contains(&key(item)) {
            continue;
        }
        match item.kind {
            MigrationKind::Image => plan.images.push(item.name.clone()),
            MigrationKind::Volume => plan.volumes.push(item.id.clone()),
            MigrationKind::Network => plan.networks.push(item.id.clone()),
            MigrationKind::Container => plan.containers.push(item.id.clone()),
        }
    }
    plan
}

/// The heading for a kind, with its count.
pub fn heading(kind: MigrationKind, n: usize) -> String {
    let word = match (kind, n) {
        (MigrationKind::Image, 1) => "image",
        (MigrationKind::Image, _) => "images",
        (MigrationKind::Volume, 1) => "volume",
        (MigrationKind::Volume, _) => "volumes",
        (MigrationKind::Network, 1) => "network",
        (MigrationKind::Network, _) => "networks",
        (MigrationKind::Container, 1) => "container",
        (MigrationKind::Container, _) => "containers",
    };
    format!("{n} {word}")
}

pub struct Import {
    state: AppState,
    stage: Stage,
    selected: BTreeSet<String>,
    /// The most recent progress frame, shown while copying.
    progress: Option<MigrationProgress>,
    /// Per-item problems, kept so a partial import can be explained.
    notes: Vec<String>,
}

impl Import {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = AppState::get(cx);
        watch(cx, &state.engine);
        Self {
            state,
            stage: Stage::Idle,
            selected: BTreeSet::new(),
            progress: None,
            notes: Vec::new(),
        }
    }

    fn scan(&mut self, cx: &mut Context<Self>) {
        let host = Arc::clone(&self.state.host);
        let this = cx.entity().downgrade();
        self.stage = Stage::Scanning;
        self.notes.clear();
        cx.notify();
        bridge::run(
            cx,
            async move { host.import_scan().await },
            move |scan, cx| {
                if let Some(this) = this.upgrade() {
                    this.update(cx, |this, cx| {
                        this.selected = select_all(&scan);
                        this.stage = Stage::Chosen(Box::new(scan));
                        cx.notify();
                    });
                }
            },
        );
    }

    fn import(&mut self, cx: &mut Context<Self>) {
        let Stage::Chosen(scan) = &self.stage else {
            return;
        };
        let plan = plan_from(scan, &self.selected);
        if plan.is_empty() {
            return;
        }
        let host = Arc::clone(&self.state.host);
        let state = self.state.clone();
        let this = cx.entity().downgrade();
        self.stage = Stage::Importing;
        self.progress = None;
        self.notes.clear();
        cx.notify();

        // Progress arrives as a stream so a long copy shows movement rather
        // than freezing on one spinner.
        bridge::stream(
            cx,
            move |tx| async move {
                let mut report = |p: MigrationProgress| {
                    let _ = tx.unbounded_send(p);
                };
                host.import_run(&plan, &mut report).await;
            },
            move |frame: MigrationProgress, cx| {
                if let Some(this) = this.upgrade() {
                    this.update(cx, |this, cx| {
                        if let Some(e) = &frame.error {
                            this.notes.push(format!("{}: {e}", frame.item));
                        }
                        if let Some(w) = &frame.warning {
                            this.notes.push(format!("{}: {w}", frame.item));
                        }
                        if frame.finished {
                            this.stage = Stage::Done(frame.message.clone());
                        }
                        this.progress = Some(frame);
                        cx.notify();
                    });
                }
            },
            move |cx| state.bump(cx),
        );
    }

    fn toggle(&mut self, k: String, cx: &mut Context<Self>) {
        if !self.selected.remove(&k) {
            self.selected.insert(k);
        }
        cx.notify();
    }
}

impl Render for Import {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::palette(cx);
        let mut body = Stack::new().gap(Size::Md);

        body = body.child(
            Stack::new()
                .gap(Size::Xs)
                .child(Text::new("Import from Docker").size(Size::Xl).medium())
                .child(
                    Text::new(
                        "Copy the images and containers from Docker Desktop, Colima or \
                         Rancher Desktop into the engine Hopper is running. Nothing is \
                         removed from Docker."
                            .to_string(),
                    )
                    .size(Size::Sm)
                    .dimmed(),
                ),
        );

        match &self.stage {
            Stage::Idle => {
                body = body.child(
                    Group::new().child(
                        Button::new("import-scan", "Look for Docker")
                            .size(Size::Sm)
                            .variant(Variant::Filled)
                            .color(ColorName::Blue)
                            .on_click(cx.listener(|this, _, _, cx| this.scan(cx))),
                    ),
                );
            }
            Stage::Scanning => {
                body = body.child(
                    Group::new()
                        .gap(Size::Sm)
                        .align(Align::Center)
                        .child(Loader::new().size(Size::Sm).color(ColorName::Blue))
                        .child(Text::new("Looking for another engine…").size(Size::Sm)),
                );
            }
            Stage::Chosen(scan) if !scan.available => {
                body = body.child(
                    Text::new(
                        scan.message
                            .clone()
                            .unwrap_or_else(|| "No other engine was found.".into()),
                    )
                    .size(Size::Sm),
                )
                .child(
                    Group::new().child(
                        Button::new("import-rescan", "Scan again")
                            .size(Size::Sm)
                            .variant(Variant::Light)
                            .on_click(cx.listener(|this, _, _, cx| this.scan(cx))),
                    ),
                );
            }
            Stage::Chosen(scan) => {
                if let Some(source) = &scan.source {
                    body = body.child(
                        Text::new(format!("Found {source}"))
                            .size(Size::Xs)
                            .dimmed(),
                    );
                }
                body = body.child(
                    Group::new()
                        .gap(Size::Xs)
                        .child(Badge::new(heading(MigrationKind::Image, scan.images.len())).size(Size::Sm))
                        .child(Badge::new(heading(MigrationKind::Volume, scan.volumes.len())).size(Size::Sm))
                        .child(Badge::new(heading(MigrationKind::Network, scan.networks.len())).size(Size::Sm))
                        .child(Badge::new(heading(MigrationKind::Container, scan.containers.len())).size(Size::Sm)),
                );

                let mut list = Stack::new().gap(Size::Xs);
                for item in rows(scan) {
                    let k = key(item);
                    let on = self.selected.contains(&k);
                    let detail = item.detail.clone().unwrap_or_default();
                    let label = if detail.is_empty() {
                        item.name.clone()
                    } else {
                        format!("{}  ·  {detail}", item.name)
                    };
                    list = list.child(
                        Checkbox::new(gpui::SharedString::from(k.clone()))
                            .label(label)
                            .checked(on)
                            .size(Size::Sm)
                            .on_change(cx.listener(move |this, _, _, cx| {
                                this.toggle(k.clone(), cx)
                            })),
                    );
                }
                body = body.child(
                    ScrollArea::new("import-list").max_height(320.0).child(list),
                );

                let n = self.selected.len();
                body = body.child(
                    Group::new().child(
                        Button::new(
                            "import-run",
                            if n == 0 {
                                "Nothing selected".to_string()
                            } else {
                                format!("Import {n} item{}", if n == 1 { "" } else { "s" })
                            },
                        )
                        .size(Size::Sm)
                        .variant(Variant::Filled)
                        .color(ColorName::Green)
                        .disabled(n == 0)
                        .on_click(cx.listener(|this, _, _, cx| this.import(cx))),
                    ),
                );
            }
            Stage::Importing => {
                let line = self
                    .progress
                    .as_ref()
                    .map(|p| p.message.clone())
                    .unwrap_or_else(|| "Starting…".into());
                let mut block = Stack::new().gap(Size::Sm).child(
                    Group::new()
                        .gap(Size::Sm)
                        .align(Align::Center)
                        .child(Loader::new().size(Size::Sm).color(ColorName::Blue))
                        .child(Text::new(line).size(Size::Sm)),
                );
                if let Some(p) = &self.progress {
                    if p.total > 0 {
                        block = block.child(
                            Progress::new((p.done as f32 / p.total as f32) * 100.0)
                                .size(Size::Sm)
                                .color(ColorName::Blue),
                        );
                    }
                }
                body = body.child(block);
            }
            Stage::Done(summary) => {
                body = body
                    .child(
                        Group::new()
                            .gap(Size::Sm)
                            .align(Align::Center)
                            .child(Icon::new(IconName::CircleCheck).size(Size::Md).color(ColorName::Green))
                            .child(Text::new(summary.clone()).size(Size::Sm).medium()),
                    )
                    .child(
                        Group::new().child(
                            Button::new("import-again", "Import something else")
                                .size(Size::Sm)
                                .variant(Variant::Light)
                                .on_click(cx.listener(|this, _, _, cx| this.scan(cx))),
                        ),
                    );
            }
        }

        // Whatever could not be copied exactly, said plainly rather than
        // swallowed into a success message.
        if !self.notes.is_empty() {
            let mut notes = Stack::new().gap(Size::Xs).child(
                Text::new("Worth knowing".to_string()).size(Size::Sm).medium(),
            );
            for note in &self.notes {
                notes = notes.child(Text::new(note.clone()).size(Size::Xs).dimmed());
            }
            body = body.child(notes);
        }

        // The pane is bounded by the window, and "Worth knowing" grows with
        // whatever the scan could not copy exactly — an import with a lot to
        // say ran off the bottom with no way to reach it.
        ScrollArea::new("import-scroll").fill().child(
            div().p_6().child(
                div()
                    .max_w(px(680.0))
                    .p_6()
                    .rounded_lg()
                    .bg(palette.bg_subtle)
                    .border_1()
                    .border_color(palette.border_subtle)
                    .child(body),
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(kind: MigrationKind, id: &str, name: &str) -> MigrationItem {
        MigrationItem {
            kind,
            id: id.into(),
            name: name.into(),
            detail: None,
        }
    }

    fn scan() -> MigrationScan {
        MigrationScan {
            available: true,
            images: vec![item(MigrationKind::Image, "sha1", "nginx:latest")],
            volumes: vec![item(MigrationKind::Volume, "data", "data")],
            networks: vec![item(MigrationKind::Network, "net1", "shop_default")],
            containers: vec![item(MigrationKind::Container, "c1", "web")],
            ..Default::default()
        }
    }

    #[test]
    fn a_fresh_scan_starts_fully_selected() {
        let s = scan();
        assert_eq!(select_all(&s).len(), 4, "the common case is bring it all");
    }

    #[test]
    fn an_image_and_a_volume_sharing_a_name_stay_distinct() {
        // Both are called "data"; ticking one must not tick the other.
        let a = item(MigrationKind::Image, "data", "data");
        let b = item(MigrationKind::Volume, "data", "data");
        assert_ne!(key(&a), key(&b));
    }

    #[test]
    fn the_plan_carries_each_kind_to_its_own_list() {
        let s = scan();
        let plan = plan_from(&s, &select_all(&s));
        assert_eq!(plan.images, vec!["nginx:latest".to_string()], "images travel by reference");
        assert_eq!(plan.volumes, vec!["data".to_string()]);
        assert_eq!(plan.networks, vec!["net1".to_string()]);
        assert_eq!(plan.containers, vec!["c1".to_string()]);
        assert_eq!(plan.total(), 4);
    }

    #[test]
    fn unticking_removes_only_that_item() {
        let s = scan();
        let mut sel = select_all(&s);
        sel.remove(&key(&s.images[0]));
        let plan = plan_from(&s, &sel);
        assert!(plan.images.is_empty());
        assert_eq!(plan.total(), 3);
    }

    #[test]
    fn an_empty_selection_produces_an_empty_plan() {
        let plan = plan_from(&scan(), &BTreeSet::new());
        assert!(plan.is_empty());
    }

    #[test]
    fn the_plan_pins_the_source_the_scan_found() {
        // Without this the import could be pointed at a different daemon than
        // the one the user was shown.
        let mut s = scan();
        s.source_endpoint = Some(model::MigrationEndpoint::Unix { path: "/x.sock".into() });
        let plan = plan_from(&s, &select_all(&s));
        assert_eq!(plan.source, s.source_endpoint);
    }

    #[test]
    fn headings_are_singular_when_there_is_one() {
        assert_eq!(heading(MigrationKind::Image, 1), "1 image");
        assert_eq!(heading(MigrationKind::Image, 3), "3 images");
        assert_eq!(heading(MigrationKind::Container, 0), "0 containers");
    }
}
