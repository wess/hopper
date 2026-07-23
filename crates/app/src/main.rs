//! Hopper — a native Docker desktop client built with gpui and guise.
//!
//! `main` installs the theme, wires the menu bar, and opens the root window.
//! Everything else lives in the domain crates; the async Docker layer is
//! reached through the tokio bridge.

mod bridge;
mod format;
mod root;
mod sidebar;
mod state;
mod theme;
mod views;

use gpui::prelude::*;
use gpui::{
    px, size, App, Application, Bounds, KeyBinding, Menu, MenuItem, OsAction, SharedString,
    TitlebarOptions, WindowBounds, WindowOptions,
};
use guise::prelude::*;

macro_rules! actions {
    ($($name:ident),* $(,)?) => {
        $(
            #[derive(Clone, PartialEq, Default, Debug, gpui::Action)]
            #[action(namespace = hopper, no_json)]
            pub struct $name;
        )*
    };
}

actions!(
    Quit,
    Hide,
    HideOthers,
    ShowAll,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Refresh,
    OpenSettings,
    ShowDocs,
);

fn menu(name: &'static str, items: Vec<MenuItem>) -> Menu {
    Menu {
        name: SharedString::new_static(name),
        items,
    }
}

fn menus() -> Vec<Menu> {
    vec![
        menu(
            "Hopper",
            vec![
                MenuItem::action("Settings…", OpenSettings),
                MenuItem::separator(),
                MenuItem::action("Hide Hopper", Hide),
                MenuItem::action("Hide Others", HideOthers),
                MenuItem::action("Show All", ShowAll),
                MenuItem::separator(),
                MenuItem::action("Quit Hopper", Quit),
            ],
        ),
        menu(
            "Edit",
            vec![
                MenuItem::os_action("Cut", Cut, OsAction::Cut),
                MenuItem::os_action("Copy", Copy, OsAction::Copy),
                MenuItem::os_action("Paste", Paste, OsAction::Paste),
                MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
            ],
        ),
        menu("View", vec![MenuItem::action("Refresh", Refresh)]),
        menu("Help", vec![MenuItem::action("Documentation", ShowDocs)]),
    ]
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn,app=info".into()),
        )
        .init();

    Application::new().run(|cx: &mut App| {
        theme::build(ColorScheme::Dark).init(cx);

        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-h", Hide, None),
            KeyBinding::new("alt-cmd-h", HideOthers, None),
            KeyBinding::new("cmd-,", OpenSettings, None),
            KeyBinding::new("cmd-r", Refresh, None),
        ]);
        cx.set_menus(menus());
        cx.on_action::<Quit>(|_, cx| cx.quit());
        cx.on_action::<Hide>(|_, cx| cx.hide());
        cx.on_action::<HideOthers>(|_, cx| cx.hide_other_apps());
        cx.on_action::<ShowAll>(|_, cx| cx.unhide_other_apps());
        cx.on_action::<ShowDocs>(|_, cx| cx.open_url("https://github.com/wess/hopper"));

        let bounds = Bounds::centered(None, size(px(1380.0), px(880.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(920.0), px(560.0))),
                titlebar: Some(TitlebarOptions {
                    title: Some(format!("Hopper v{}", env!("CARGO_PKG_VERSION")).into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| cx.new(root::Root::new),
        )
        .unwrap();
        cx.activate(true);
    });
}
