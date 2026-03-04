mod app;
mod config;
mod copy_mode;
mod ink_workaround;
mod pomodoro;
mod response_timer;
#[allow(dead_code)]
mod selection;
mod tab;
mod url;
mod zoom;

fn main() {
    let config = config::Config::load();
    let font_size = config.font_size;
    let font_family = config.font_family.clone();

    let window_size = config.window_size();
    let window_position = config.window_position();

    growterm_macos::run(window_size, window_position, move |window, rx| {
        // GpuDrawer must be created on the main thread (Metal requirement)
        let (width, height) = window.inner_size();
        let font_path = resolve_font_path(&font_family);
        let drawer = growterm_gpu_draw::GpuDrawer::new(window.clone(), width, height, font_size, font_path.as_deref());

        let config = config.clone();
        std::thread::spawn(move || {
            app::run(window, rx, drawer, config);
        });
    });
}

/// Resolve a font family name to a file path.
/// Returns None if it's the built-in font or the path can't be found.
fn resolve_font_path(family: &str) -> Option<String> {
    if family == "FiraCodeNerdFontMono-Retina" || family.is_empty() {
        return None;
    }

    // Try as direct file path first
    if std::path::Path::new(family).exists() {
        return Some(family.to_string());
    }

    // Try to find via macOS font system using core-text
    // Search common font directories
    let home = std::env::var("HOME").unwrap_or_default();
    let search_dirs = [
        format!("{home}/Library/Fonts"),
        "/Library/Fonts".to_string(),
        "/System/Library/Fonts".to_string(),
    ];

    for dir in &search_dirs {
        let dir_path = std::path::Path::new(dir);
        if let Ok(entries) = std::fs::read_dir(dir_path) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.contains(family) {
                    return Some(entry.path().to_string_lossy().to_string());
                }
            }
        }
    }

    None
}
