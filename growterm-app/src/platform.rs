#[cfg(target_os = "linux")]
pub use growterm_linux::*;

#[cfg(target_os = "macos")]
pub use growterm_macos::*;
