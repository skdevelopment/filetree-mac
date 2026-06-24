pub mod app;
pub mod app_logic;
pub mod charts;
pub mod delete;
pub mod export;
pub mod fda;
pub mod macos_dir;
pub mod menu;
pub mod models;
pub mod paths;
pub mod platform;
pub mod progress;
pub mod progress_ui;
pub mod scan_bridge;
pub mod scan_cache;
pub mod scan_progress;
pub mod scan_traverse;
pub mod scanner;
pub mod session;
pub mod theme;
pub mod tree_state;
pub mod ui;
pub mod util;

pub use menu::ViewMode;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");