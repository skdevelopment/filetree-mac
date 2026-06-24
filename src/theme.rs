//! Color themes for the TUI.
//!
//! A [`Theme`] maps *semantic UI roles* (borders, headers, the selected row,
//! danger/warning accents, …) to concrete [`Color`]s. Render code asks the
//! theme for a role — e.g. `theme.danger_style()` — instead of hardcoding a
//! literal color, so the entire palette can be swapped at runtime without
//! touching the rendering logic.
//!
//! Themes are plain [`Copy`] value types, so the active theme is stored inline
//! on the app and cycled with no allocation.

use ratatui::style::{Color, Modifier, Style};

/// A named palette mapping UI roles to colors.
///
/// Each field is a role rather than a literal color. To add a theme, append a
/// `const` value below and list it in [`THEMES`]; the rest of the UI picks it
/// up automatically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// Human-readable name; also the value accepted by `--theme`.
    pub name: &'static str,
    /// Border color for the main panels (tree, chart, status, lists).
    pub border: Color,
    /// Foreground for table headers (always rendered bold).
    pub header: Color,
    /// Background of the selected table row.
    pub selection_bg: Color,
    /// Foreground of the selected table row.
    pub selection_fg: Color,
    /// Background of the filter bar.
    pub filter_bg: Color,
    /// Foreground of the filter bar.
    pub filter_fg: Color,
    /// Accent for informational dialogs (help, path input, export).
    pub accent: Color,
    /// Accent for destructive confirmations (delete).
    pub danger: Color,
    /// Accent for warnings (full disk access, scan errors).
    pub warning: Color,
}

impl Theme {
    /// Style for default panel borders.
    pub fn border_style(&self) -> Style {
        Style::default().fg(self.border)
    }

    /// Style for table header rows (bold).
    pub fn header_style(&self) -> Style {
        Style::default()
            .fg(self.header)
            .add_modifier(Modifier::BOLD)
    }

    /// Style for the highlighted (selected) table row.
    pub fn selection_style(&self) -> Style {
        Style::default()
            .bg(self.selection_bg)
            .fg(self.selection_fg)
            .add_modifier(Modifier::BOLD)
    }

    /// Style for the filter bar.
    pub fn filter_style(&self) -> Style {
        Style::default().bg(self.filter_bg).fg(self.filter_fg)
    }

    /// Background + default foreground for the menu bar and toolbar rows.
    pub fn chrome_bar_style(&self) -> Style {
        self.filter_style()
    }

    /// A clickable label on the menu bar or toolbar.
    ///
    /// Inactive buttons use [`filter_fg`] (paired with [`filter_bg`]); active
    /// buttons use the selection palette so the highlight is always distinct
    /// from the bar background.
    pub fn chrome_button_style(&self, active: bool) -> Style {
        if active {
            Style::default()
                .bg(self.selection_bg)
                .fg(self.selection_fg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(self.filter_fg)
                .add_modifier(Modifier::BOLD)
        }
    }

    /// Style for informational accents (dialog borders).
    pub fn accent_style(&self) -> Style {
        Style::default().fg(self.accent)
    }

    /// Style for destructive accents (delete confirmations).
    pub fn danger_style(&self) -> Style {
        Style::default().fg(self.danger)
    }

    /// Style for warning accents (FDA banner, scan errors).
    pub fn warning_style(&self) -> Style {
        Style::default().fg(self.warning)
    }

    /// Looks up a built-in theme by name, case-insensitively.
    pub fn from_name(name: &str) -> Option<Theme> {
        THEMES
            .iter()
            .copied()
            .find(|t| t.name.eq_ignore_ascii_case(name))
    }

    /// The next theme in the built-in cycle, wrapping around.
    pub fn next(&self) -> Theme {
        self.step(1)
    }

    /// The previous theme in the built-in cycle, wrapping around.
    pub fn prev(&self) -> Theme {
        self.step(-1)
    }

    fn step(&self, delta: isize) -> Theme {
        let len = THEMES.len() as isize;
        let idx = THEMES.iter().position(|t| t.name == self.name).unwrap_or(0) as isize;
        // Rust's `%` keeps the sign of the dividend, so add `len` before the
        // final modulo to stay non-negative when stepping backwards.
        let next = ((idx + delta) % len + len) % len;
        THEMES[next as usize]
    }
}

impl Default for Theme {
    fn default() -> Self {
        CLASSIC
    }
}

/// All built-in themes, in cycle order. `--theme` and the runtime cycle key
/// resolve against this list.
pub const THEMES: &[Theme] = &[
    CLASSIC,
    NORD,
    GRUVBOX,
    SOLARIZED,
    DRACULA,
    TOKYO_NIGHT,
    CATPPUCCIN,
    ONE_DARK,
    MONOKAI,
    MONOCHROME,
    LIGHT,
];

/// The original look: terminal-default borders with ANSI accent colors.
pub const CLASSIC: Theme = Theme {
    name: "classic",
    border: Color::Reset,
    header: Color::Reset,
    selection_bg: Color::White,
    selection_fg: Color::Black,
    filter_bg: Color::Blue,
    filter_fg: Color::White,
    accent: Color::Cyan,
    danger: Color::Red,
    warning: Color::Yellow,
};

/// Nord — cool, muted blues. <https://www.nordtheme.com/>
pub const NORD: Theme = Theme {
    name: "nord",
    border: Color::Rgb(0x81, 0xA1, 0xC1),
    header: Color::Rgb(0x88, 0xC0, 0xD0),
    // Darker than `filter_bg` so the active chrome button reads on the bar.
    selection_bg: Color::Rgb(0x4C, 0x56, 0x6A),
    selection_fg: Color::Rgb(0xEC, 0xEF, 0xF4),
    filter_bg: Color::Rgb(0x5E, 0x81, 0xAC),
    filter_fg: Color::Rgb(0xEC, 0xEF, 0xF4),
    accent: Color::Rgb(0x88, 0xC0, 0xD0),
    danger: Color::Rgb(0xBF, 0x61, 0x6A),
    warning: Color::Rgb(0xEB, 0xCB, 0x8B),
};

/// Gruvbox — warm, retro earth tones. <https://github.com/morhetz/gruvbox>
pub const GRUVBOX: Theme = Theme {
    name: "gruvbox",
    border: Color::Rgb(0x92, 0x83, 0x74),
    header: Color::Rgb(0xFA, 0xBD, 0x2F),
    selection_bg: Color::Rgb(0x66, 0x5C, 0x54),
    selection_fg: Color::Rgb(0xFB, 0xF1, 0xC7),
    filter_bg: Color::Rgb(0x45, 0x85, 0x88),
    filter_fg: Color::Rgb(0xFB, 0xF1, 0xC7),
    accent: Color::Rgb(0x83, 0xA5, 0x98),
    danger: Color::Rgb(0xFB, 0x49, 0x34),
    warning: Color::Rgb(0xFA, 0xBD, 0x2F),
};

/// Solarized Dark — low-contrast, balanced. <https://ethanschoonover.com/solarized/>
pub const SOLARIZED: Theme = Theme {
    name: "solarized",
    border: Color::Rgb(0x58, 0x6E, 0x75),
    header: Color::Rgb(0x2A, 0xA1, 0x98),
    selection_bg: Color::Rgb(0x07, 0x36, 0x42),
    selection_fg: Color::Rgb(0x93, 0xA1, 0xA1),
    filter_bg: Color::Rgb(0x26, 0x8B, 0xD2),
    filter_fg: Color::Rgb(0xFD, 0xF6, 0xE3),
    accent: Color::Rgb(0x2A, 0xA1, 0x98),
    danger: Color::Rgb(0xDC, 0x32, 0x2F),
    warning: Color::Rgb(0xB5, 0x89, 0x00),
};

/// Dracula — vivid on a dark slate. <https://draculatheme.com/>
pub const DRACULA: Theme = Theme {
    name: "dracula",
    border: Color::Rgb(0x62, 0x72, 0xA4),
    // Headers use the near-white Dracula foreground rather than the purple
    // accent: purple-on-dark headers read poorly. Purple stays as the filter
    // accent below, where it sits behind dark text.
    header: Color::Rgb(0xF8, 0xF8, 0xF2),
    selection_bg: Color::Rgb(0x44, 0x47, 0x5A),
    selection_fg: Color::Rgb(0xF8, 0xF8, 0xF2),
    filter_bg: Color::Rgb(0xBD, 0x93, 0xF9),
    filter_fg: Color::Rgb(0x28, 0x2A, 0x36),
    accent: Color::Rgb(0x8B, 0xE9, 0xFD),
    danger: Color::Rgb(0xFF, 0x55, 0x55),
    warning: Color::Rgb(0xF1, 0xFA, 0x8C),
};

/// Tokyo Night — deep indigo with bright blue accents. <https://github.com/enkia/tokyo-night-vscode-theme>
pub const TOKYO_NIGHT: Theme = Theme {
    name: "tokyo-night",
    border: Color::Rgb(0x56, 0x5F, 0x89),
    header: Color::Rgb(0xC0, 0xCA, 0xF5),
    selection_bg: Color::Rgb(0x28, 0x34, 0x57),
    selection_fg: Color::Rgb(0xC0, 0xCA, 0xF5),
    filter_bg: Color::Rgb(0x7A, 0xA2, 0xF7),
    filter_fg: Color::Rgb(0x1A, 0x1B, 0x26),
    accent: Color::Rgb(0x7D, 0xCF, 0xFF),
    danger: Color::Rgb(0xF7, 0x76, 0x8E),
    warning: Color::Rgb(0xE0, 0xAF, 0x68),
};

/// Catppuccin Mocha — soft pastel on warm dark. <https://catppuccin.com/>
pub const CATPPUCCIN: Theme = Theme {
    name: "catppuccin",
    border: Color::Rgb(0x6C, 0x70, 0x86),
    header: Color::Rgb(0xCD, 0xD6, 0xF4),
    selection_bg: Color::Rgb(0x31, 0x32, 0x44),
    selection_fg: Color::Rgb(0xCD, 0xD6, 0xF4),
    filter_bg: Color::Rgb(0x89, 0xB4, 0xFA),
    filter_fg: Color::Rgb(0x1E, 0x1E, 0x2E),
    accent: Color::Rgb(0x89, 0xDC, 0xEB),
    danger: Color::Rgb(0xF3, 0x8B, 0xA8),
    warning: Color::Rgb(0xF9, 0xE2, 0xAF),
};

/// One Dark — the Atom classic. <https://github.com/atom/one-dark-syntax>
pub const ONE_DARK: Theme = Theme {
    name: "one-dark",
    border: Color::Rgb(0x4B, 0x52, 0x63),
    header: Color::Rgb(0xFF, 0xFF, 0xFF),
    selection_bg: Color::Rgb(0x3E, 0x44, 0x51),
    selection_fg: Color::Rgb(0xFF, 0xFF, 0xFF),
    filter_bg: Color::Rgb(0x61, 0xAF, 0xEF),
    filter_fg: Color::Rgb(0x28, 0x2C, 0x34),
    accent: Color::Rgb(0x56, 0xB6, 0xC2),
    danger: Color::Rgb(0xE0, 0x6C, 0x75),
    warning: Color::Rgb(0xE5, 0xC0, 0x7B),
};

/// Monokai — high-energy neon on espresso. <https://monokai.pro/>
pub const MONOKAI: Theme = Theme {
    name: "monokai",
    border: Color::Rgb(0x75, 0x71, 0x5E),
    header: Color::Rgb(0xA6, 0xE2, 0x2E),
    selection_bg: Color::Rgb(0x49, 0x48, 0x3E),
    selection_fg: Color::Rgb(0xF8, 0xF8, 0xF2),
    filter_bg: Color::Rgb(0x66, 0xD9, 0xEF),
    filter_fg: Color::Rgb(0x27, 0x28, 0x22),
    accent: Color::Rgb(0x66, 0xD9, 0xEF),
    danger: Color::Rgb(0xF9, 0x26, 0x72),
    warning: Color::Rgb(0xE6, 0xDB, 0x74),
};

/// Light — for light-background terminals (GitHub-style palette).
pub const LIGHT: Theme = Theme {
    name: "light",
    border: Color::Rgb(0xAF, 0xB8, 0xC1),
    header: Color::Rgb(0x09, 0x69, 0xDA),
    selection_bg: Color::Rgb(0x09, 0x69, 0xDA),
    selection_fg: Color::Rgb(0xFF, 0xFF, 0xFF),
    filter_bg: Color::Rgb(0xDD, 0xF4, 0xFF),
    filter_fg: Color::Rgb(0x0A, 0x30, 0x69),
    accent: Color::Rgb(0x09, 0x69, 0xDA),
    danger: Color::Rgb(0xCF, 0x22, 0x2E),
    warning: Color::Rgb(0x9A, 0x67, 0x00),
};

/// Monochrome — grayscale only, for low-color terminals and minimal setups.
pub const MONOCHROME: Theme = Theme {
    name: "monochrome",
    border: Color::DarkGray,
    header: Color::White,
    selection_bg: Color::Gray,
    selection_fg: Color::Black,
    filter_bg: Color::DarkGray,
    filter_fg: Color::White,
    accent: Color::Gray,
    danger: Color::White,
    warning: Color::Gray,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_classic() {
        assert_eq!(Theme::default(), CLASSIC);
        assert_eq!(Theme::default().name, "classic");
    }

    #[test]
    fn from_name_is_case_insensitive() {
        assert_eq!(Theme::from_name("nord"), Some(NORD));
        assert_eq!(Theme::from_name("NORD"), Some(NORD));
        assert_eq!(Theme::from_name("Gruvbox"), Some(GRUVBOX));
        assert_eq!(Theme::from_name("does-not-exist"), None);
    }

    #[test]
    fn next_and_prev_wrap_around() {
        let first = THEMES[0];
        let last = THEMES[THEMES.len() - 1];
        assert_eq!(first.prev(), last);
        assert_eq!(last.next(), first);
    }

    #[test]
    fn next_then_prev_is_identity() {
        for theme in THEMES {
            assert_eq!(theme.next().prev(), *theme);
        }
    }

    #[test]
    fn all_theme_names_are_unique() {
        for (i, a) in THEMES.iter().enumerate() {
            for b in &THEMES[i + 1..] {
                assert_ne!(a.name, b.name, "duplicate theme name: {}", a.name);
            }
        }
    }

    /// True when two colors are the same concrete RGB/ANSI value.
    fn colors_equal(a: Color, b: Color) -> bool {
        a == b
    }

    #[test]
    fn chrome_palette_has_contrast() {
        for theme in THEMES {
            assert!(
                !colors_equal(theme.filter_fg, theme.filter_bg),
                "theme '{}': filter_fg must differ from filter_bg",
                theme.name
            );
            assert!(
                !colors_equal(theme.selection_bg, theme.filter_bg),
                "theme '{}': selection_bg must differ from filter_bg (chrome highlight)",
                theme.name
            );
        }
    }

    #[test]
    fn cycle_visits_every_theme_once() {
        let mut seen = Vec::new();
        let mut theme = THEMES[0];
        for _ in 0..THEMES.len() {
            seen.push(theme.name);
            theme = theme.next();
        }
        assert_eq!(theme, THEMES[0], "cycle should return to the start");
        assert_eq!(seen.len(), THEMES.len());
    }
}
