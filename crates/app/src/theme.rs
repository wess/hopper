//! Hopper's theme, mapped onto guise.
//!
//! Docker blue is the accent, matching the design system the TypeScript build
//! established. Every visual is read from the theme so light/dark switching
//! stays free.

use gpui::Hsla;
use guise::prelude::*;
use guise::theme::{Color, Shades};

/// The neutral ramp (`0` … `9`), light to dark.
const DARK_RAMP: [&str; 10] = [
    "#C9D1D9", "#B1BAC4", "#8B949E", "#6E7681", "#484F58", "#30363D", "#21262D", "#161B22",
    "#0D1117", "#010409",
];

pub fn build(scheme: ColorScheme) -> Theme {
    let mut theme = match scheme {
        ColorScheme::Dark => Theme::dark(),
        ColorScheme::Light => Theme::light(),
    };
    theme
        .palette
        .set_shades(ColorName::Dark, Shades(DARK_RAMP.map(Color::hex)));
    theme.primary_color = ColorName::Blue;
    theme.default_radius = Size::Md;
    theme.font_family = ".SystemUIFont".into();
    theme
}

/// The monospace family for logs, terminals, and inspect output.
#[allow(dead_code)]
pub const MONO_FAMILY: &str = "Menlo";

// The full token set; panels adopt them as they land.
#[allow(dead_code)]
#[derive(Clone, Copy)]
pub struct Palette {
    pub bg_surface: Hsla,
    pub bg_subtle: Hsla,
    pub bg_muted: Hsla,
    pub border: Hsla,
    pub border_subtle: Hsla,
    pub text_muted: Hsla,
    pub row_stripe: Hsla,
}

fn hex(code: &str) -> Hsla {
    Color::hex(code).hsla()
}

pub fn colors(theme: &Theme) -> Palette {
    let shade = |i: usize| theme.color(ColorName::Dark, i).hsla();
    let gray = |i: usize| theme.color(ColorName::Gray, i).hsla();
    match theme.scheme {
        ColorScheme::Dark => Palette {
            bg_surface: shade(8),
            bg_subtle: shade(7),
            bg_muted: shade(6),
            border: shade(5),
            border_subtle: shade(6),
            text_muted: shade(2),
            row_stripe: gpui::hsla(0.0, 0.0, 1.0, 0.03),
        },
        ColorScheme::Light => Palette {
            bg_surface: hex("#ffffff"),
            bg_subtle: hex("#f6f8fa"),
            bg_muted: hex("#eaeef2"),
            border: gray(3),
            border_subtle: gray(2),
            text_muted: gray(6),
            row_stripe: gpui::hsla(0.0, 0.0, 0.0, 0.02),
        },
    }
}

pub fn palette(cx: &gpui::App) -> Palette {
    colors(guise::theme::theme(cx))
}

/// The accent for a container state, so the dot, badge, and row agree.
pub fn state_color(state: model::ContainerState) -> ColorName {
    use model::ContainerState::*;
    match state {
        Running => ColorName::Green,
        Paused => ColorName::Yellow,
        Restarting => ColorName::Blue,
        Created => ColorName::Gray,
        Removing => ColorName::Orange,
        Exited | Dead => ColorName::Red,
    }
}

/// Health has its own accent: a running-but-unhealthy container must not read
/// as green.
pub fn health_color(health: model::Health) -> Option<ColorName> {
    use model::Health::*;
    match health {
        None => Option::None,
        Starting => Some(ColorName::Yellow),
        Healthy => Some(ColorName::Green),
        Unhealthy => Some(ColorName::Red),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{ContainerState, Health};

    #[test]
    fn every_container_state_has_a_distinct_enough_accent() {
        assert_eq!(state_color(ContainerState::Running), ColorName::Green);
        assert_eq!(state_color(ContainerState::Exited), ColorName::Red);
        assert_eq!(state_color(ContainerState::Paused), ColorName::Yellow);
    }

    #[test]
    fn an_unhealthy_container_gets_a_warning_accent_of_its_own() {
        assert_eq!(health_color(Health::Unhealthy), Some(ColorName::Red));
        assert_eq!(health_color(Health::Healthy), Some(ColorName::Green));
        // No healthcheck means no badge at all, rather than a misleading one.
        assert_eq!(health_color(Health::None), None);
    }
}
