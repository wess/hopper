//! Cross-view state. `AppState` lives for the whole app and is provided as
//! context by `Root`; views read the signals they care about and watch them.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{Entity, SharedString};
use guise::prelude::*;
use host::Host;
use model::{Container, EngineStatus, Image, Network, Settings, Volume};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    Dashboard,
    Containers,
    Images,
    Registry,
    Volumes,
    Networks,
    Stacks,
    Import,
    Settings,
}

impl Route {
    pub fn label(&self) -> &'static str {
        match self {
            Route::Dashboard => "Dashboard",
            Route::Containers => "Containers",
            Route::Images => "Images",
            Route::Registry => "Registry",
            Route::Volumes => "Volumes",
            Route::Networks => "Networks",
            Route::Stacks => "Stacks",
            Route::Import => "Import",
            Route::Settings => "Settings",
        }
    }

    pub fn icon(&self) -> IconName {
        match self {
            Route::Dashboard => IconName::LayoutDashboard,
            Route::Containers => IconName::Box,
            Route::Images => IconName::Layers,
            Route::Registry => IconName::PackageSearch,
            Route::Volumes => IconName::Database,
            Route::Networks => IconName::Network,
            Route::Stacks => IconName::Boxes,
            Route::Import => IconName::Import,
            Route::Settings => IconName::Settings,
        }
    }

    /// Parse a route from its label, for the `HOPPER_ROUTE` dev override.
    pub fn from_env() -> Option<Route> {
        let want = std::env::var("HOPPER_ROUTE").ok()?;
        Route::all().into_iter().find(|r| r.label().eq_ignore_ascii_case(want.trim()))
    }

    /// Sidebar order.
    pub fn all() -> [Route; 9] {
        [
            Route::Dashboard,
            Route::Containers,
            Route::Images,
            Route::Registry,
            Route::Volumes,
            Route::Networks,
            Route::Stacks,
            Route::Import,
            Route::Settings,
        ]
    }
}

/// How a list request finished, so views can tell "empty" from "not loaded"
/// from "failed" — three states that must not render the same way.
#[derive(Clone, Debug, PartialEq)]
pub enum Load<T> {
    Loading,
    Ready(T),
    Failed(String),
}

impl<T> Load<T> {
    pub fn ready(&self) -> Option<&T> {
        match self {
            Load::Ready(v) => Some(v),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn is_loading(&self) -> bool {
        matches!(self, Load::Loading)
    }

    #[allow(dead_code)]
    pub fn error(&self) -> Option<&str> {
        match self {
            Load::Failed(e) => Some(e),
            _ => None,
        }
    }
}

// `settings` and `selection` are the contract the settings pane and bulk
// actions consume; they are staged here so views can adopt them in place.
#[allow(dead_code)]
#[derive(Clone)]
pub struct AppState {
    pub host: Arc<Host>,
    pub route: Signal<Route>,
    pub settings: Signal<Settings>,
    pub engine: Signal<EngineStatus>,

    pub containers: Signal<Load<Vec<Container>>>,
    pub images: Signal<Load<Vec<Image>>>,
    pub volumes: Signal<Load<Vec<Volume>>>,
    pub networks: Signal<Load<Vec<Network>>>,

    /// Show stopped containers as well as running ones.
    pub show_all: Signal<bool>,
    pub search: Signal<String>,
    pub selection: Signal<BTreeSet<String>>,
    /// The container whose detail pane is open, if any.
    pub selected: Signal<Option<Container>>,
    /// Whether the navigation rail is collapsed to icons only.
    pub sidebar_collapsed: Signal<bool>,

    /// The image reference the Run dialog is open for, if any. Setting this
    /// opens the dialog; the Images and Registry views both trigger it.
    pub run_target: Signal<Option<String>>,

    /// App-wide transient feedback, painted top-right over everything. Views
    /// push to it through [`AppState::toast`] so an action always shows the
    /// user that something happened.
    pub toasts: Entity<ToastStack>,

    /// Bumped to ask the active view to refetch. The Docker event stream
    /// bumps this, which is how the UI stays live without polling every list.
    pub epoch: Signal<u64>,
}

impl AppState {
    pub fn get(cx: &gpui::App) -> AppState {
        use_context::<AppState>(cx).expect("AppState provided by Root")
    }

    pub fn new(host: Arc<Host>, cx: &mut gpui::App) -> Self {
        let settings = host.settings();
        Self {
            host,
            route: Signal::new(cx, Route::from_env().unwrap_or(Route::Containers)),
            settings: Signal::new(cx, settings),
            engine: Signal::new(cx, EngineStatus::default()),
            containers: Signal::new(cx, Load::Loading),
            images: Signal::new(cx, Load::Loading),
            volumes: Signal::new(cx, Load::Loading),
            networks: Signal::new(cx, Load::Loading),
            show_all: Signal::new(cx, true),
            search: Signal::new(cx, String::new()),
            selection: Signal::new(cx, BTreeSet::new()),
            selected: Signal::new(cx, None),
            sidebar_collapsed: Signal::new(cx, std::env::var("HOPPER_SIDEBAR").as_deref() == Ok("collapsed")),
            run_target: Signal::new(cx, None),
            toasts: cx.new(|_| ToastStack::new().duration(Some(Duration::from_secs(4)))),
            epoch: Signal::new(cx, 0),
        }
    }

    pub fn bump(&self, cx: &mut gpui::App) {
        self.epoch.update(cx, |n| *n += 1);
    }

    /// Show a titled, coloured toast — for successes and failures that deserve
    /// more than a plain line.
    pub fn toast_titled(
        &self,
        cx: &mut gpui::App,
        title: impl Into<SharedString>,
        message: impl Into<SharedString>,
        color: ColorName,
    ) {
        self.toasts.update(cx, |ts, cx| {
            ts.push_titled(title, message, color, cx);
        });
    }

    /// Open the Run dialog for an image reference.
    pub fn run_image(&self, cx: &mut gpui::App, reference: impl Into<String>) {
        self.run_target.set(cx, Some(reference.into()));
    }
}

/// Filter a container list by the search box. Matching name, image, and id
/// means the same box works for all three, which is what users try.
pub fn filter_containers<'a>(list: &'a [Container], query: &str) -> Vec<&'a Container> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return list.iter().collect();
    }
    list.iter()
        .filter(|c| {
            c.name.to_lowercase().contains(&q)
                || c.image.to_lowercase().contains(&q)
                || c.id.starts_with(&q)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{ContainerState, Health};
    use std::collections::BTreeMap;

    fn container(name: &str, image: &str, id: &str) -> Container {
        Container {
            id: id.into(),
            name: name.into(),
            image: image.into(),
            image_id: String::new(),
            command: String::new(),
            created: 0,
            state: ContainerState::Running,
            status: String::new(),
            health: Health::None,
            ports: vec![],
            labels: BTreeMap::new(),
            mounts: vec![],
            networks: vec![],
            compose_project: None,
            compose_service: None,
        }
    }

    #[test]
    fn an_empty_query_keeps_everything() {
        let list = vec![container("web", "nginx", "abc")];
        assert_eq!(filter_containers(&list, "   ").len(), 1);
    }

    #[test]
    fn search_matches_name_image_or_id_prefix() {
        let list = vec![
            container("web", "nginx:latest", "abc123"),
            container("db", "postgres:16", "def456"),
        ];
        assert_eq!(filter_containers(&list, "web").len(), 1);
        assert_eq!(filter_containers(&list, "postgres").len(), 1);
        assert_eq!(filter_containers(&list, "abc").len(), 1);
        assert_eq!(filter_containers(&list, "zzz").len(), 0);
    }

    #[test]
    fn search_is_case_insensitive() {
        let list = vec![container("Web", "NGINX", "abc")];
        assert_eq!(filter_containers(&list, "web").len(), 1);
        assert_eq!(filter_containers(&list, "nginx").len(), 1);
    }

    #[test]
    fn load_states_are_distinguishable() {
        let loading: Load<Vec<u8>> = Load::Loading;
        assert!(loading.is_loading());
        assert!(loading.ready().is_none());

        let empty: Load<Vec<u8>> = Load::Ready(vec![]);
        assert!(!empty.is_loading());
        // An empty result is ready, not loading — the view must say "none yet",
        // not spin forever.
        assert_eq!(empty.ready().map(|v| v.len()), Some(0));

        let failed: Load<Vec<u8>> = Load::Failed("boom".into());
        assert_eq!(failed.error(), Some("boom"));
        assert!(failed.ready().is_none());
    }
}
