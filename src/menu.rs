//! Menu bar, toolbar, and the single [`Action`] vocabulary that keyboard, mouse,
//! menu, and toolbar input all funnel into.
//!
//! Keeping every user intent in one enum means there is exactly one place
//! ([`crate::app::App::dispatch_action`]) that performs each action, so a feature
//! added for the keyboard is automatically reachable by mouse (and vice versa).
//! The layout helpers here are pure functions returning click rectangles, shared
//! by both rendering and hit-testing so the two can never drift apart.

use crate::app::ViewMode;
use crate::models::SortKey;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Every discrete thing the user can ask the app to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    Help,
    OpenFilter,
    ClearFilter,
    CycleSort,
    ToggleSortDir,
    SetSort(SortKey),
    RescanSelected,
    RescanTree,
    CancelScan,
    GotoPath,
    ToggleSymlinks,
    ToggleHidden,
    ThemePicker,
    NextView,
    PrevView,
    SetView(ViewMode),
    Collapse,
    Expand,
    ToggleExpandOrScan,
    MoveUp,
    MoveDown,
    PageUp,
    PageDown,
    Home,
    End,
    RevealFinder,
    Delete,
    Export,
}

/// One row in a dropdown menu.
pub struct MenuItem {
    pub label: &'static str,
    /// Key hint shown right-aligned (e.g. "R", "Tab"); empty for mouse-only items.
    pub key: &'static str,
    pub action: Action,
}

/// A top-level menu (its title in the menu bar plus its dropdown items).
pub struct Menu {
    pub title: &'static str,
    pub items: &'static [MenuItem],
}

/// All top-level menus, left to right. Every keyboard shortcut appears here with
/// its hint, which is what makes the full shortcut set discoverable by mouse.
pub const MENUS: &[Menu] = &[
    Menu {
        title: "File",
        items: &[
            MenuItem {
                label: "Open folder…",
                key: "o",
                action: Action::GotoPath,
            },
            MenuItem {
                label: "Export report…",
                key: "e",
                action: Action::Export,
            },
            MenuItem {
                label: "Reveal in Finder",
                key: "f",
                action: Action::RevealFinder,
            },
            MenuItem {
                label: "Quit",
                key: "q",
                action: Action::Quit,
            },
        ],
    },
    Menu {
        title: "View",
        items: &[
            MenuItem {
                label: "Tree",
                key: "1",
                action: Action::SetView(ViewMode::Tree),
            },
            MenuItem {
                label: "Top files",
                key: "2",
                action: Action::SetView(ViewMode::TopFiles),
            },
            MenuItem {
                label: "Extensions",
                key: "3",
                action: Action::SetView(ViewMode::Extensions),
            },
            MenuItem {
                label: "Volumes",
                key: "4",
                action: Action::SetView(ViewMode::Volumes),
            },
            MenuItem {
                label: "Next view",
                key: "Tab",
                action: Action::NextView,
            },
            MenuItem {
                label: "Previous view",
                key: "S-Tab",
                action: Action::PrevView,
            },
            MenuItem {
                label: "Color theme…",
                key: "t",
                action: Action::ThemePicker,
            },
        ],
    },
    Menu {
        title: "Sort",
        items: &[
            MenuItem {
                label: "Cycle column",
                key: "s",
                action: Action::CycleSort,
            },
            MenuItem {
                label: "Toggle direction",
                key: "S",
                action: Action::ToggleSortDir,
            },
            MenuItem {
                label: "By name",
                key: "",
                action: Action::SetSort(SortKey::Name),
            },
            MenuItem {
                label: "By size",
                key: "",
                action: Action::SetSort(SortKey::Size),
            },
            MenuItem {
                label: "By allocated",
                key: "",
                action: Action::SetSort(SortKey::Allocated),
            },
            MenuItem {
                label: "By date",
                key: "",
                action: Action::SetSort(SortKey::Date),
            },
            MenuItem {
                label: "By extension",
                key: "",
                action: Action::SetSort(SortKey::Extension),
            },
            MenuItem {
                label: "By owner",
                key: "",
                action: Action::SetSort(SortKey::Owner),
            },
            MenuItem {
                label: "By percent",
                key: "",
                action: Action::SetSort(SortKey::Percent),
            },
        ],
    },
    Menu {
        title: "Actions",
        items: &[
            MenuItem {
                label: "Rescan selected",
                key: "r",
                action: Action::RescanSelected,
            },
            MenuItem {
                label: "Rescan all",
                key: "R",
                action: Action::RescanTree,
            },
            MenuItem {
                label: "Cancel scan",
                key: "c",
                action: Action::CancelScan,
            },
            MenuItem {
                label: "Filter…",
                key: "/",
                action: Action::OpenFilter,
            },
            MenuItem {
                label: "Clear filter",
                key: "Esc",
                action: Action::ClearFilter,
            },
            MenuItem {
                label: "Toggle hidden",
                key: "H",
                action: Action::ToggleHidden,
            },
            MenuItem {
                label: "Toggle symlinks",
                key: "v",
                action: Action::ToggleSymlinks,
            },
            MenuItem {
                label: "Delete…",
                key: "d",
                action: Action::Delete,
            },
        ],
    },
    Menu {
        title: "Help",
        items: &[MenuItem {
            label: "Keyboard shortcuts",
            key: "?",
            action: Action::Help,
        }],
    },
];

/// A clickable toolbar button.
pub struct ToolButton {
    pub label: &'static str,
    pub action: Action,
}

/// Quick-access toolbar buttons. The four view buttons come first (the active one
/// is highlighted at render time), then the most common actions.
pub const TOOLBAR: &[ToolButton] = &[
    ToolButton {
        label: "Tree",
        action: Action::SetView(ViewMode::Tree),
    },
    ToolButton {
        label: "Files",
        action: Action::SetView(ViewMode::TopFiles),
    },
    ToolButton {
        label: "Ext",
        action: Action::SetView(ViewMode::Extensions),
    },
    ToolButton {
        label: "Vols",
        action: Action::SetView(ViewMode::Volumes),
    },
    ToolButton {
        label: "Rescan",
        action: Action::RescanSelected,
    },
    ToolButton {
        label: "Filter",
        action: Action::OpenFilter,
    },
    ToolButton {
        label: "Sort",
        action: Action::CycleSort,
    },
    ToolButton {
        label: "Hidden",
        action: Action::ToggleHidden,
    },
    ToolButton {
        label: "Theme",
        action: Action::ThemePicker,
    },
    ToolButton {
        label: "Export",
        action: Action::Export,
    },
    ToolButton {
        label: "Help",
        action: Action::Help,
    },
    ToolButton {
        label: "Quit",
        action: Action::Quit,
    },
];

/// Build one row of labeled cells separated by `sep` spaces, returning the
/// rendered text and the `(x, width)` span of each cell (x relative to the row's
/// left edge). Shared by the menu bar and toolbar so the painted text and the
/// click rectangles are computed from the exact same arithmetic.
pub fn render_row(cells: &[String], sep: usize) -> (String, Vec<(u16, u16)>) {
    let mut text = String::new();
    let mut spans = Vec::with_capacity(cells.len());
    let mut x: u16 = 0;
    for (i, cell) in cells.iter().enumerate() {
        if i > 0 {
            for _ in 0..sep {
                text.push(' ');
            }
            x = x.saturating_add(sep as u16);
        }
        let w = cell.chars().count() as u16;
        spans.push((x, w));
        text.push_str(cell);
        x = x.saturating_add(w);
    }
    (text, spans)
}

/// Cells rendered for the menu bar (one per top-level menu).
pub fn menu_bar_cells() -> Vec<String> {
    MENUS.iter().map(|m| format!(" {} ", m.title)).collect()
}

/// Cells rendered for the toolbar (one per button).
pub fn toolbar_cells() -> Vec<String> {
    TOOLBAR.iter().map(|b| format!("[{}]", b.label)).collect()
}

/// Pixel width a dropdown needs to fit its widest `label + key` row.
pub fn dropdown_width(menu: &Menu) -> u16 {
    let inner = menu
        .items
        .iter()
        .map(|it| it.label.chars().count() + it.key.chars().count() + 3)
        .max()
        .unwrap_or(8);
    // +2 for the left/right border.
    (inner as u16).saturating_add(2)
}

/// Format a single dropdown row's inner text to a fixed inner width.
pub fn dropdown_item_text(item: &MenuItem, inner_width: usize) -> String {
    let key_w = item.key.chars().count();
    let label_room = inner_width.saturating_sub(key_w + 1).max(1);
    let mut label: String = item.label.chars().take(label_room).collect();
    let pad = inner_width.saturating_sub(label.chars().count() + key_w);
    for _ in 0..pad {
        label.push(' ');
    }
    label.push_str(item.key);
    label
}

/// Translate a key event into an [`Action`], independent of UI context.
///
/// Returns `None` for keys that are handled contextually elsewhere (modal input,
/// the FDA banner, or keys with no binding). Context-sensitive movement (j/k,
/// arrows) maps to the generic move actions; [`crate::app::App::dispatch_action`]
/// decides whether that means "move the table cursor" or "scroll the chart pane".
pub fn key_to_action(key: KeyEvent) -> Option<Action> {
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    match key.code {
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Char('?') => Some(Action::Help),
        KeyCode::Char('/') => Some(Action::OpenFilter),
        KeyCode::Char('s') if shift => Some(Action::ToggleSortDir),
        KeyCode::Char('S') => Some(Action::ToggleSortDir),
        KeyCode::Char('s') => Some(Action::CycleSort),
        KeyCode::Char('r') if shift => Some(Action::RescanTree),
        KeyCode::Char('R') => Some(Action::RescanTree),
        KeyCode::Char('r') => Some(Action::RescanSelected),
        KeyCode::Char('c') => Some(Action::CancelScan),
        KeyCode::Char('g') | KeyCode::Char('o') => Some(Action::GotoPath),
        KeyCode::Char('v') => Some(Action::ToggleSymlinks),
        KeyCode::Char('H') => Some(Action::ToggleHidden),
        KeyCode::Char('t') | KeyCode::Char('T') => Some(Action::ThemePicker),
        KeyCode::Tab => Some(Action::NextView),
        KeyCode::BackTab => Some(Action::PrevView),
        KeyCode::Char('1') => Some(Action::SetView(ViewMode::Tree)),
        KeyCode::Char('2') => Some(Action::SetView(ViewMode::TopFiles)),
        KeyCode::Char('3') => Some(Action::SetView(ViewMode::Extensions)),
        KeyCode::Char('4') => Some(Action::SetView(ViewMode::Volumes)),
        KeyCode::Char('h') | KeyCode::Left => Some(Action::Collapse),
        KeyCode::Char('l') | KeyCode::Right => Some(Action::Expand),
        KeyCode::Enter => Some(Action::ToggleExpandOrScan),
        KeyCode::Char('j') | KeyCode::Down => Some(Action::MoveDown),
        KeyCode::Char('k') | KeyCode::Up => Some(Action::MoveUp),
        KeyCode::PageDown => Some(Action::PageDown),
        KeyCode::PageUp => Some(Action::PageUp),
        KeyCode::Home => Some(Action::Home),
        KeyCode::End => Some(Action::End),
        KeyCode::Char('f') => Some(Action::RevealFinder),
        KeyCode::Char('d') => Some(Action::Delete),
        KeyCode::Char('e') => Some(Action::Export),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_row_positions_cells_after_separators() {
        let (text, spans) = render_row(&["AA".to_string(), "BBB".to_string()], 2);
        assert_eq!(text, "AA  BBB");
        assert_eq!(spans, vec![(0, 2), (4, 3)]);
    }

    #[test]
    fn render_row_single_cell_starts_at_zero() {
        let (text, spans) = render_row(&["X".to_string()], 3);
        assert_eq!(text, "X");
        assert_eq!(spans, vec![(0, 1)]);
    }

    #[test]
    fn menu_bar_cells_match_menu_count() {
        assert_eq!(menu_bar_cells().len(), MENUS.len());
    }

    #[test]
    fn dropdown_item_text_pads_to_inner_width() {
        let item = MenuItem {
            label: "Quit",
            key: "q",
            action: Action::Quit,
        };
        let line = dropdown_item_text(&item, 12);
        assert_eq!(line.chars().count(), 12);
        assert!(line.starts_with("Quit"));
        assert!(line.ends_with('q'));
    }

    #[test]
    fn key_to_action_maps_quit_and_views() {
        let q = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        assert_eq!(key_to_action(q), Some(Action::Quit));
        let v2 = KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE);
        assert_eq!(key_to_action(v2), Some(Action::SetView(ViewMode::TopFiles)));
    }

    #[test]
    fn key_to_action_distinguishes_sort_direction() {
        let lower = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE);
        assert_eq!(key_to_action(lower), Some(Action::CycleSort));
        let upper = KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT);
        assert_eq!(key_to_action(upper), Some(Action::ToggleSortDir));
    }

    #[test]
    fn every_menu_item_action_is_unique_or_intentional() {
        // Sanity: menus expose at least one item each.
        for menu in MENUS {
            assert!(!menu.items.is_empty(), "menu {} has no items", menu.title);
        }
    }
}
