//! Adaptive layout maths, deliberately kept out of the UI crate.
//!
//! The breakpoints and the grid solver are pure functions of the window size,
//! which means the "does the layout break at 719 px?" question is answered by a
//! unit test rather than by launching the app.

use serde::{Deserialize, Serialize};

/// Width classes in logical (DPI-independent) points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Breakpoint {
    /// Phone-shaped / snapped window. Navigation becomes a bottom bar.
    Compact,
    /// Half-screen desktop window. Navigation is a narrow icon rail.
    #[default]
    Medium,
    /// Full desktop window. Persistent labelled rail, multi-column detail view.
    Expanded,
}

impl Breakpoint {
    pub const COMPACT_MAX: f32 = 720.0;
    pub const MEDIUM_MAX: f32 = 1200.0;

    pub fn for_width(width: f32) -> Self {
        if width < Self::COMPACT_MAX {
            Breakpoint::Compact
        } else if width <= Self::MEDIUM_MAX {
            Breakpoint::Medium
        } else {
            Breakpoint::Expanded
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Breakpoint::Compact => "Compact",
            Breakpoint::Medium => "Medium",
            Breakpoint::Expanded => "Expanded",
        }
    }

    /// Where the primary navigation lives, which is what actually changes on
    /// resize. Keeping this a property of the breakpoint means every screen
    /// reflows consistently.
    pub fn navigation(self) -> NavMode {
        match self {
            Breakpoint::Compact => NavMode::BottomBar,
            Breakpoint::Medium => NavMode::Rail,
            Breakpoint::Expanded => NavMode::RailWithLabels,
        }
    }

    pub fn all() -> [Breakpoint; 3] {
        [
            Breakpoint::Compact,
            Breakpoint::Medium,
            Breakpoint::Expanded,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavMode {
    BottomBar,
    Rail,
    RailWithLabels,
}

/// Everything the UI needs, computed once per frame from the window size.
#[derive(Debug, Clone, Copy)]
pub struct LayoutMetrics {
    pub breakpoint: Breakpoint,
    pub nav: NavMode,
    /// Width reserved by the left navigation rail (`0` in compact mode).
    pub rail_width: f32,
    /// Height reserved by the bottom navigation bar.
    pub bottom_bar_height: f32,
    pub content_padding: f32,
    pub gutter: f32,
    pub card_width: f32,
    pub poster_height: f32,
    pub columns: usize,
    /// True when the details view has room for a side-by-side hero.
    pub two_column_details: bool,
    /// True when cards can show a title underneath without truncation.
    pub show_card_titles: bool,
}

impl Default for LayoutMetrics {
    fn default() -> Self {
        Self::compute(1200.0, 800.0, 1.0, None)
    }
}

impl LayoutMetrics {
    /// `forced` exists for the settings screen's layout preview; production
    /// callers pass `None` and let the window size decide.
    pub fn compute(width: f32, height: f32, dpi: f32, forced: Option<Breakpoint>) -> Self {
        // Work in logical points: everything above winit is DPI-scaled, and
        // hard-coding physical pixels breaks at 125 %/150 %.
        let width = width.max(1.0);
        let height = height.max(1.0);
        let dpi = if dpi.is_finite() && dpi > 0.0 {
            dpi
        } else {
            1.0
        };
        let logical_width = width / dpi;
        let logical_height = height / dpi;

        let breakpoint = forced.unwrap_or_else(|| Breakpoint::for_width(logical_width));
        let nav = breakpoint.navigation();

        let (rail_width, bottom_bar_height, content_padding, gutter) = match breakpoint {
            Breakpoint::Compact => (0.0, 56.0, 12.0, 10.0),
            Breakpoint::Medium => (68.0, 0.0, 16.0, 12.0),
            Breakpoint::Expanded => (208.0, 0.0, 24.0, 16.0),
        };

        let available = (logical_width - rail_width - content_padding * 2.0).max(120.0);
        let min_card = match breakpoint {
            Breakpoint::Compact => 132.0,
            Breakpoint::Medium => 148.0,
            Breakpoint::Expanded => 168.0,
        };

        // Solve for the column count that maximises card width without dropping
        // below `min_card`, then stretch cards to fill the row exactly.
        let mut columns = ((available + gutter) / (min_card + gutter)).floor() as i32;
        columns = columns.clamp(1, 8);
        let columns = columns as usize;
        let card_width =
            ((available - gutter * (columns.saturating_sub(1) as f32)) / columns as f32).max(96.0);

        let poster_height = match breakpoint {
            Breakpoint::Compact => card_width * 1.42,
            _ => card_width * 1.5,
        };

        Self {
            breakpoint,
            nav,
            rail_width,
            bottom_bar_height,
            content_padding,
            gutter,
            card_width,
            poster_height,
            columns,
            two_column_details: breakpoint == Breakpoint::Expanded && logical_height > 520.0,
            show_card_titles: breakpoint != Breakpoint::Compact,
        }
    }

    /// Usable content width inside the navigation chrome.
    pub fn content_width(&self, width: f32) -> f32 {
        (width - self.rail_width - self.content_padding * 2.0).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breakpoints_match_documented_boundaries() {
        assert_eq!(Breakpoint::for_width(719.0), Breakpoint::Compact);
        assert_eq!(Breakpoint::for_width(720.0), Breakpoint::Medium);
        assert_eq!(Breakpoint::for_width(1200.0), Breakpoint::Medium);
        assert_eq!(Breakpoint::for_width(1201.0), Breakpoint::Expanded);
    }

    #[test]
    fn navigation_chrome_follows_breakpoint() {
        let compact = LayoutMetrics::compute(600.0, 900.0, 1.0, None);
        assert_eq!(compact.nav, NavMode::BottomBar);
        assert_eq!(compact.rail_width, 0.0);
        assert!(compact.bottom_bar_height > 0.0);

        let wide = LayoutMetrics::compute(1600.0, 900.0, 1.0, None);
        assert_eq!(wide.nav, NavMode::RailWithLabels);
        assert_eq!(wide.bottom_bar_height, 0.0);
    }

    #[test]
    fn grid_never_overflows_its_container() {
        for width in [420.0, 719.0, 900.0, 1280.0, 1920.0, 2560.0, 3840.0] {
            let m = LayoutMetrics::compute(width, 900.0, 1.0, None);
            let available = m.content_width(width);
            let used = m.card_width * m.columns as f32 + m.gutter * (m.columns - 1) as f32;
            assert!(
                used <= available + 1.0,
                "width {width}: used {used} > available {available}"
            );
            assert!(m.columns >= 1 && m.columns <= 8);
            assert!(m.card_width >= 96.0);
        }
    }

    #[test]
    fn high_dpi_keeps_same_column_count() {
        let a = LayoutMetrics::compute(1600.0, 900.0, 1.0, None);
        let b = LayoutMetrics::compute(2400.0, 1350.0, 1.5, None);
        assert_eq!(a.columns, b.columns);
        assert_eq!(a.breakpoint, b.breakpoint);
    }

    #[test]
    fn forced_breakpoint_wins() {
        let m = LayoutMetrics::compute(1900.0, 900.0, 1.0, Some(Breakpoint::Compact));
        assert_eq!(m.breakpoint, Breakpoint::Compact);
        assert_eq!(m.rail_width, 0.0);
    }
}
