//! The container file browser — Docker Desktop's Files tab.
//!
//! Surfaces the `archive` module: browse a container's filesystem, and copy
//! files out. This is the "get a file out of a container" operation that had
//! no equivalent before, and that people reach for daily.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{div, Context, Window};
use guise::prelude::*;

use crate::bridge;
use crate::format;
use crate::state::AppState;
use crate::theme;
use model::FileEntry;

/// The parent of an absolute directory path, or `None` at the root.
///
/// Pure so the breadcrumb's "up" logic is tested without a live container.
pub fn parent_dir(cwd: &str) -> Option<String> {
    if cwd == "/" || cwd.is_empty() {
        return None;
    }
    let trimmed = cwd.trim_end_matches('/');
    Some(match trimmed.rfind('/') {
        Some(0) | None => "/".to_string(),
        Some(i) => trimmed[..i].to_string(),
    })
}

pub struct Files {
    state: AppState,
    container: String,
    /// The directory being shown, always absolute.
    cwd: String,
    entries: Vec<FileEntry>,
    error: Option<String>,
    loading: bool,
}

impl Files {
    pub fn new(container: String, cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            state: AppState::get(cx),
            container,
            cwd: "/".to_string(),
            entries: Vec::new(),
            error: None,
            loading: true,
        };
        view.load(cx);
        view
    }

    /// Point the browser at a different container, resetting to its root.
    pub fn show(&mut self, container: String, cx: &mut Context<Self>) {
        if container == self.container {
            return;
        }
        self.container = container;
        self.cwd = "/".to_string();
        self.entries.clear();
        self.load(cx);
        cx.notify();
    }

    fn navigate(&mut self, dir: String, cx: &mut Context<Self>) {
        self.cwd = dir;
        self.load(cx);
        cx.notify();
    }

    /// The parent of the current directory, or `None` at the root.
    fn parent(&self) -> Option<String> {
        parent_dir(&self.cwd)
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let host = Arc::clone(&self.state.host);
        let id = self.container.clone();
        let dir = self.cwd.clone();
        self.loading = true;
        let entity = cx.entity().downgrade();
        bridge::run(
            cx,
            async move { host.container_ls(&id, &dir).await },
            move |result, cx| {
                let _ = entity.update(cx, |this: &mut Self, cx| {
                    this.loading = false;
                    match result {
                        Ok(entries) => {
                            this.entries = entries;
                            this.error = None;
                        }
                        Err(e) => this.error = Some(e.message),
                    }
                    cx.notify();
                });
            },
        );
    }

    fn row(&self, entry: &FileEntry, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::palette(cx);
        let is_dir = entry.dir;
        let path = entry.path.clone();

        let name = div()
            .flex_1()
            .child(
                Group::new()
                    .gap(Size::Xs)
                    .align(Align::Center)
                    .child(
                        Text::new(if is_dir { "📁" } else { "📄" }.to_string())
                            .size(Size::Xs),
                    )
                    .child(Text::new(entry.name.clone()).size(Size::Xs)),
            );

        let meta = Text::new(if is_dir {
            entry.mode.clone()
        } else {
            format!("{}  ·  {}", format::bytes(entry.size), entry.mode)
        })
        .size(Size::Xs)
        .dimmed();

        let mut row = div()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .px_3()
            .py_1()
            .border_b_1()
            .border_color(palette.border_subtle)
            .child(name)
            .child(meta);

        if is_dir {
            // A directory is clickable to descend into it.
            row = row.cursor_pointer().on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, _, _, cx| this.navigate(path.clone(), cx)),
            );
        }
        row
    }
}

impl Render for Files {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::palette(cx);

        // Breadcrumb: the current path, with an "up" affordance.
        let mut breadcrumb = Group::new().gap(Size::Xs).align(Align::Center).child(
            Text::new(self.cwd.clone()).size(Size::Xs).dimmed(),
        );
        if let Some(parent) = self.parent() {
            breadcrumb = breadcrumb.child(
                Button::new("files-up", "↑ up")
                    .size(Size::Xs)
                    .variant(Variant::Subtle)
                    .on_click(cx.listener(move |this, _, _, cx| this.navigate(parent.clone(), cx))),
            );
        }

        let body = if let Some(err) = self.error.clone() {
            // A container with no shell cannot be browsed; say so plainly.
            crate::views::failure("Could not read this directory", &err)
        } else if self.loading && self.entries.is_empty() {
            crate::views::message("Loading…")
        } else if self.entries.is_empty() {
            crate::views::message("This directory is empty.")
        } else {
            let mut list = div().flex().flex_col();
            for entry in &self.entries {
                list = list.child(self.row(entry, cx));
            }
            list.into_any_element()
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .child(
                div()
                    .flex()
                    .items_center()
                    .p_2()
                    .border_b_1()
                    .border_color(palette.border_subtle)
                    .child(breadcrumb),
            )
            .child(div().flex_1().overflow_hidden().child(body))
    }
}

#[cfg(test)]
mod tests {
    use super::parent_dir;

    #[test]
    fn the_root_has_no_parent() {
        assert_eq!(parent_dir("/"), None);
        assert_eq!(parent_dir(""), None);
    }

    #[test]
    fn a_nested_directory_walks_up_one_level() {
        assert_eq!(parent_dir("/etc/nginx"), Some("/etc".to_string()));
        // A trailing slash must not produce an empty component.
        assert_eq!(parent_dir("/etc/nginx/"), Some("/etc".to_string()));
    }

    #[test]
    fn a_top_level_directory_walks_up_to_root() {
        assert_eq!(parent_dir("/etc"), Some("/".to_string()));
    }
}
