use clap::Parser;
use filetree::app::run_app;
use filetree::platform::default_scan_path;
use filetree::theme::{Theme, THEMES};
use filetree::VERSION;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "filetree",
    about = "TreeSize replica for macOS — interactive disk usage TUI",
    version = VERSION
)]
struct Args {
    /// Path to scan (default: / on macOS, ~ elsewhere)
    path: Option<PathBuf>,

    /// Color theme: classic, nord, gruvbox, solarized, dracula, monochrome
    #[arg(long, default_value = "classic")]
    theme: String,
}

fn main() {
    let args = Args::parse();

    let theme = match Theme::from_name(&args.theme) {
        Some(theme) => theme,
        None => {
            let names = THEMES.iter().map(|t| t.name).collect::<Vec<_>>().join(", ");
            eprintln!(
                "filetree: unknown theme '{}'. Available themes: {names}",
                args.theme
            );
            std::process::exit(2);
        }
    };

    let path = args.path.unwrap_or_else(default_scan_path);
    if let Err(e) = run_app(Some(path), theme) {
        eprintln!("filetree failed: {e}");
        std::process::exit(1);
    }
}
