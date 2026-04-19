mod app;
mod config;
mod copy_mode;
mod search_mode;
mod pomodoro;
mod platform;
mod response_timer;
#[allow(dead_code)]
mod selection;
mod tab;
mod url;
mod zoom;

fn main() {
    // Install panic hook to log panics with backtrace to file
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        let log_path = config::config_dir().join("panic.log");
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let thread = std::thread::current();
        let thread_name = thread.name().unwrap_or("<unnamed>");
        let content = format!(
            "[unix:{timestamp}] thread '{thread_name}' {info}\n\nBacktrace:\n{backtrace}\n"
        );
        let _ = std::fs::OpenOptions::new()
            .create(true).append(true).open(&log_path)
            .and_then(|mut f| {
                use std::io::Write;
                f.write_all(content.as_bytes())
            });
        default_hook(info);
    }));

    let config = config::Config::load();
    let font_size = config.font_size;
    let font_family = config.font_family.clone();

    let window_size = config.window_size();
    let window_position = config.window_position();

    crate::platform::run(window_size, window_position, move |window, rx| {
        // GpuDrawer must be created on the UI thread for the platform window backend.
        let (width, height) = window.inner_size();
        let font_path = resolve_font_path(&font_family);
        let drawer = growterm_gpu_draw::GpuDrawer::new(window.clone(), width, height, font_size, font_path.as_deref());

        let config = config.clone();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                app::run(window.clone(), rx, drawer, config);
            }));
            if let Err(e) = result {
                let msg = if let Some(s) = e.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = e.downcast_ref::<String>() {
                    s.clone()
                } else {
                    format!("{e:?}")
                };
                let log_path = config::config_dir().join("panic.log");
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let _ = std::fs::OpenOptions::new()
                    .create(true).append(true).open(&log_path)
                    .and_then(|mut f| {
                        use std::io::Write;
                        writeln!(f, "[unix:{timestamp}] app thread panicked (catch_unwind): {msg}")
                    });
                eprintln!("app thread panicked: {msg}");
                std::process::exit(1);
            }
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
