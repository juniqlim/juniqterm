use std::collections::HashSet;
use std::process::{Command, Stdio};
use std::time::Duration;

/// Build the growterm binary and return the path.
pub fn build_binary() -> String {
    let output = Command::new("cargo")
        .args(["build", "--package", "growterm-app"])
        .output()
        .expect("failed to run cargo build");
    assert!(
        output.status.success(),
        "cargo build failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let metadata = Command::new("cargo")
        .args(["metadata", "--format-version=1", "--no-deps"])
        .output()
        .expect("failed to run cargo metadata");
    let meta: serde_json::Value =
        serde_json::from_slice(&metadata.stdout).expect("invalid cargo metadata json");
    let target_dir = meta["target_directory"].as_str().unwrap();
    format!("{target_dir}/debug/growterm")
}

fn spawn_command(bin: &str) -> Command {
    let mut cmd = Command::new(bin);
    cmd.env("GROWTERM_DISABLE_APP_RELAUNCH", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

/// Parse the dump file format, returning (cursor_row, cursor_col, grid_rows).
pub fn parse_dump(content: &str) -> (u16, u16, Vec<String>) {
    let mut lines = content.lines();

    let cursor_line = lines.next().expect("missing cursor line");
    let cursor_part = cursor_line.strip_prefix("cursor:").expect("bad cursor line");
    let mut parts = cursor_part.split(',');
    let row: u16 = parts.next().unwrap().parse().unwrap();
    let col: u16 = parts.next().unwrap().parse().unwrap();

    let grid_header = lines.next().expect("missing grid header");
    assert_eq!(grid_header, "grid:");

    let rows: Vec<String> = lines.map(|l| l.to_string()).collect();
    (row, col, rows)
}

/// Parse the dump file, returning only grid rows (ignoring cursor).
pub fn parse_dump_rows(content: &str) -> Vec<String> {
    let (_, _, rows) = parse_dump(content);
    rows
}

/// Poll for dump file with timeout. Optionally checks if child has crashed.
pub fn wait_for_dump(
    dump_path: &std::path::Path,
    timeout: Duration,
    mut child: Option<&mut std::process::Child>,
) -> Option<String> {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(200));
        if dump_path.exists() {
            if let Ok(content) = std::fs::read_to_string(dump_path) {
                if !content.is_empty() {
                    return Some(content);
                }
            }
        }
        if let Some(ref mut c) = child {
            if let Ok(Some(_)) = c.try_wait() {
                return None;
            }
        }
    }
    None
}

/// Kill child process and remove dump file.
pub fn cleanup(child: &mut std::process::Child, dump_path: &std::path::Path) {
    #[cfg(unix)]
    kill_process_tree(child.id());

    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file(dump_path);
}

#[cfg(unix)]
fn kill_process_tree(root_pid: u32) {
    let mut descendants = collect_descendants(root_pid);
    descendants.sort_unstable();
    descendants.dedup();

    for pid in &descendants {
        send_signal(*pid, "-TERM");
    }
    send_signal(root_pid, "-TERM");
    std::thread::sleep(Duration::from_millis(100));

    for pid in &descendants {
        send_signal(*pid, "-KILL");
    }
    send_signal(root_pid, "-KILL");
}

#[cfg(unix)]
fn collect_descendants(root_pid: u32) -> Vec<u32> {
    let mut visited: HashSet<u32> = HashSet::new();
    let mut stack = vec![root_pid];
    let mut descendants = Vec::new();

    while let Some(pid) = stack.pop() {
        for child in child_pids(pid) {
            if visited.insert(child) {
                descendants.push(child);
                stack.push(child);
            }
        }
    }

    descendants
}

#[cfg(unix)]
fn child_pids(parent_pid: u32) -> Vec<u32> {
    let output = match Command::new("pgrep")
        .args(["-P", &parent_pid.to_string()])
        .output()
    {
        Ok(output) => output,
        Err(_) => return Vec::new(),
    };

    if !output.status.success() {
        return Vec::new();
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .collect()
}

#[cfg(unix)]
fn send_signal(pid: u32, signal: &str) {
    let _ = Command::new("kill")
        .args([signal, &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Spawn growterm with GROWTERM_GRID_DUMP set.
pub fn spawn_with_dump(bin: &str, dump_path: &std::path::Path) -> std::process::Child {
    spawn_command(bin)
        .env("GROWTERM_GRID_DUMP", dump_path)
        .spawn()
        .expect("failed to launch growterm")
}

/// Spawn growterm with GROWTERM_GRID_DUMP and GROWTERM_TEST_INPUT set.
pub fn spawn_with_dump_and_input(
    bin: &str,
    dump_path: &std::path::Path,
    test_input: &str,
) -> std::process::Child {
    spawn_command(bin)
        .env("GROWTERM_GRID_DUMP", dump_path)
        .env("GROWTERM_TEST_INPUT", test_input)
        .spawn()
        .expect("failed to launch growterm")
}

/// Spawn growterm with no extra test envs.
pub fn spawn_app(bin: &str) -> std::process::Child {
    spawn_command(bin)
        .spawn()
        .expect("failed to launch growterm")
}

/// Check if the `claude` CLI is installed and available in PATH.
pub fn has_claude_cli() -> bool {
    Command::new("sh")
        .arg("-lc")
        .arg("command -v claude >/dev/null 2>&1")
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Check if a Korean input source is currently selected on macOS.
pub fn korean_input_source_selected() -> bool {
    let output = match Command::new("defaults")
        .arg("read")
        .arg("com.apple.HIToolbox")
        .arg("AppleSelectedInputSources")
        .output()
    {
        Ok(output) => output,
        Err(_) => return false,
    };
    if !output.status.success() {
        return false;
    }
    String::from_utf8_lossy(&output.stdout).contains("com.apple.inputmethod.Korean")
}

/// Activate a process by PID using AppleScript (bring to front).
pub fn activate_by_pid(pid: u32) {
    let script = format!(
        r#"tell application "System Events"
            set frontmost of (first process whose unix id is {pid}) to true
        end tell"#
    );
    let _ = Command::new("osascript").arg("-e").arg(&script).output();
    std::thread::sleep(Duration::from_millis(500));
}
