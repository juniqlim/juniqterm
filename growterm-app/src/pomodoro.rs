use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

const WORK_SECS: u64 = 25 * 60;
const BREAK_SECS: u64 = 3 * 60;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase {
    Idle,
    Working,
    Break,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TickResult {
    Noop,
    StartedBreak,
}

pub struct Pomodoro {
    enabled: bool,
    phase: Phase,
    started_at: Option<Instant>,
    /// Tab index → scrollback length at the moment Working phase started.
    scrollback_snapshot: HashMap<u64, usize>,
    /// AI coaching response lines, shared with the background thread.
    ai_response: Arc<Mutex<Option<Vec<String>>>>,
}

impl Pomodoro {
    pub fn new() -> Self {
        Self {
            enabled: false,
            phase: Phase::Idle,
            started_at: None,
            scrollback_snapshot: HashMap::new(),
            ai_response: Arc::new(Mutex::new(None)),
        }
    }

    pub fn toggle(&mut self) {
        self.enabled = !self.enabled;
        if !self.enabled {
            self.phase = Phase::Idle;
            self.started_at = None;
            self.scrollback_snapshot.clear();
            *self.ai_response.lock().unwrap() = None;
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    #[cfg(test)]
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Returns the scrollback snapshot (tab index → scrollback length at work start).
    pub fn scrollback_snapshot(&self) -> &HashMap<u64, usize> {
        &self.scrollback_snapshot
    }

    /// Called when user types. Starts work timer if idle.
    /// `tab_scrollback_lens` provides current scrollback length per tab index.
    pub fn on_input(&mut self, tab_scrollback_lens: &[(u64, usize)]) {
        self.on_input_at(Instant::now(), tab_scrollback_lens);
    }

    fn on_input_at(&mut self, now: Instant, tab_scrollback_lens: &[(u64, usize)]) {
        if !self.enabled {
            return;
        }
        if self.phase == Phase::Idle {
            self.phase = Phase::Working;
            self.started_at = Some(now);
            self.scrollback_snapshot.clear();
            *self.ai_response.lock().unwrap() = None;
            for &(tab_idx, sb_len) in tab_scrollback_lens {
                self.scrollback_snapshot.insert(tab_idx, sb_len);
            }
        }
    }

    /// Called periodically. Transitions state if time elapsed.
    pub fn tick(&mut self) -> TickResult {
        self.tick_at(Instant::now())
    }

    fn tick_at(&mut self, now: Instant) -> TickResult {
        if !self.enabled {
            return TickResult::Noop;
        }
        let started = match self.started_at {
            Some(t) => t,
            None => return TickResult::Noop,
        };
        let elapsed = now.duration_since(started).as_secs();
        match self.phase {
            Phase::Working => {
                if elapsed >= WORK_SECS {
                    self.phase = Phase::Break;
                    self.started_at = Some(now);
                    return TickResult::StartedBreak;
                }
            }
            Phase::Break => {
                if elapsed >= BREAK_SECS {
                    self.phase = Phase::Idle;
                    self.started_at = None;
                    self.scrollback_snapshot.clear();
                }
            }
            Phase::Idle => {}
        }
        TickResult::Noop
    }

    pub fn is_input_blocked(&self) -> bool {
        self.enabled && self.phase == Phase::Break
    }

    /// Set the AI coaching response from the background thread handle.
    pub fn set_ai_response(&self, lines: Vec<String>) {
        *self.ai_response.lock().unwrap() = Some(lines);
    }

    /// Get a clone of the Arc for the background thread to write into.
    pub fn ai_response_handle(&self) -> Arc<Mutex<Option<Vec<String>>>> {
        Arc::clone(&self.ai_response)
    }

    pub fn coaching_lines(&self) -> Option<Vec<String>> {
        if self.phase != Phase::Break {
            return None;
        }
        let guard = self.ai_response.lock().unwrap();
        if let Some(ref lines) = *guard {
            let mut result = vec!["[Coaching]".to_string(), String::new()];
            result.extend(lines.iter().cloned());
            Some(result)
        } else {
            Some(vec![
                "[Coaching]".to_string(),
                String::new(),
                "분석 중...".to_string(),
            ])
        }
    }

    /// Returns display text for the timer, or None if idle.
    pub fn display_text(&self) -> Option<String> {
        self.display_text_at(Instant::now())
    }

    fn display_text_at(&self, now: Instant) -> Option<String> {
        if !self.enabled {
            return None;
        }
        let started = self.started_at?;
        let elapsed = now.duration_since(started).as_secs();
        match self.phase {
            Phase::Working => {
                let remaining = WORK_SECS.saturating_sub(elapsed);
                let m = remaining / 60;
                let s = remaining % 60;
                Some(format!("\u{1F345} {m:02}:{s:02}"))
            }
            Phase::Break => {
                let remaining = BREAK_SECS.saturating_sub(elapsed);
                let m = remaining / 60;
                let s = remaining % 60;
                Some(format!("\u{2615} {m:02}:{s:02}"))
            }
            Phase::Idle => None,
        }
    }
}

/// Spawn a background thread that calls the coaching command with tab_text via stdin.
///
/// If `coaching_command` is Some, it is executed as a shell command with tab_text piped to stdin.
/// Otherwise the default `claude -p` with a built-in prompt is used.
pub fn spawn_ai_coaching(
    tab_text: String,
    ai_response: Arc<Mutex<Option<Vec<String>>>>,
    coaching_command: Option<String>,
) {
    std::thread::spawn(move || {
        use std::io::Write;
        let default_system = "아래는 터미널에서 25분간 작업한 내용입니다. 탭별로 구분되어 있습니다. \
            당신은 코치입니다. 판단하거나 가르치지 마세요. \
            관찰한 내용을 짧게 알려주고, 사용자가 미처 보지 못했을 부분을 질문으로 던져주세요. \
            한국어로 3-4문장 이내로 답하세요.";
        let cmd = coaching_command.unwrap_or_else(|| {
            let claude_path = find_claude_path();
            format!("{claude_path} --system '{default_system}' -p")
        });
        let mut child = match std::process::Command::new("sh")
            .args(["-c", &cmd])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                *ai_response.lock().unwrap() = Some(vec![format!("AI 호출 실패: {e}")]);
                return;
            }
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(tab_text.as_bytes());
        }
        let result = child.wait_with_output();
        let lines = match result {
            Ok(output) if output.status.success() && !output.stdout.is_empty() => {
                let text = String::from_utf8_lossy(&output.stdout);
                text.lines().map(|l| l.to_string()).collect()
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let msg = if stderr.trim().is_empty() {
                    format!("AI 호출 실패: exit code {}", output.status)
                } else {
                    format!("AI 호출 실패: {}", stderr.trim())
                };
                vec![msg]
            }
            Err(e) => {
                vec![format!("AI 호출 실패: {e}")]
            }
        };
        save_coaching_file(&coaching_dir(), &lines);
        *ai_response.lock().unwrap() = Some(lines);
    });
}

fn find_claude_path() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let local_bin = format!("{home}/.local/bin/claude");
    if std::path::Path::new(&local_bin).exists() {
        return local_bin;
    }
    "claude".to_string()
}

fn coaching_dir() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    std::path::PathBuf::from(home)
        .join(".config")
        .join("growterm")
        .join("coaching")
}

fn save_coaching_file(dir: &std::path::Path, lines: &[String]) {
    if let Err(e) = std::fs::create_dir_all(dir) {
        eprintln!("Failed to create coaching dir: {e}");
        return;
    }
    let (date_filename, timestamp) = local_date_and_timestamp();
    let path = dir.join(date_filename);
    let mut entry = format!("\n## {timestamp}\n\n");
    entry.push_str(&lines.join("\n"));
    entry.push('\n');

    use std::fs::OpenOptions;
    use std::io::Write;
    match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(mut f) => {
            if let Err(e) = f.write_all(entry.as_bytes()) {
                eprintln!("Failed to save coaching file: {e}");
            }
        }
        Err(e) => eprintln!("Failed to save coaching file: {e}"),
    }
}

/// Returns (daily_filename, full_timestamp) e.g. ("20260304.md", "2026-03-04 13:11:51")
fn local_date_and_timestamp() -> (String, String) {
    use std::process::Command;
    let date_out = Command::new("date").arg("+%Y%m%d").output();
    let ts_out = Command::new("date").arg("+%Y-%m-%d %H:%M:%S").output();
    let date = date_out
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            secs.to_string()
        });
    let timestamp = ts_out
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| date.clone());
    (format!("{date}.md"), timestamp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn enabled_pomodoro() -> Pomodoro {
        let mut p = Pomodoro::new();
        p.toggle();
        p
    }

    #[test]
    fn initial_state_is_disabled() {
        let p = Pomodoro::new();
        assert!(!p.is_enabled());
        assert!(!p.is_input_blocked());
        assert!(p.display_text_at(Instant::now()).is_none());
    }

    #[test]
    fn toggle_enables_and_disables() {
        let mut p = Pomodoro::new();
        assert!(!p.is_enabled());
        p.toggle();
        assert!(p.is_enabled());
        p.toggle();
        assert!(!p.is_enabled());
    }

    #[test]
    fn toggle_off_resets_state() {
        let mut p = enabled_pomodoro();
        let now = Instant::now();
        p.on_input_at(now, &[(0, 100)]);
        assert_eq!(p.phase(), Phase::Working);

        p.toggle(); // disable
        assert_eq!(p.phase(), Phase::Idle);
        assert!(!p.is_input_blocked());
        assert!(p.scrollback_snapshot.is_empty());
    }

    #[test]
    fn on_input_ignored_when_disabled() {
        let mut p = Pomodoro::new(); // disabled
        let now = Instant::now();
        p.on_input_at(now, &[(0, 50)]);
        assert_eq!(p.phase(), Phase::Idle);
        assert!(p.scrollback_snapshot.is_empty());
    }

    #[test]
    fn on_input_starts_working_and_snapshots_scrollback() {
        let mut p = enabled_pomodoro();
        let now = Instant::now();
        p.on_input_at(now, &[(0, 100), (1, 200)]);
        assert_eq!(p.phase(), Phase::Working);
        assert!(!p.is_input_blocked());
        assert_eq!(p.scrollback_snapshot[&0], 100);
        assert_eq!(p.scrollback_snapshot[&1], 200);
    }

    #[test]
    fn on_input_during_working_is_noop() {
        let mut p = enabled_pomodoro();
        let now = Instant::now();
        p.on_input_at(now, &[(0, 100)]);
        let before = p.started_at;
        p.on_input_at(now + Duration::from_secs(10), &[(0, 150)]);
        assert_eq!(p.started_at, before);
        // Snapshot should not change
        assert_eq!(p.scrollback_snapshot[&0], 100);
    }

    #[test]
    fn tick_returns_started_break_on_transition() {
        let mut p = enabled_pomodoro();
        let now = Instant::now();
        p.on_input_at(now, &[(0, 50)]);

        let result = p.tick_at(now + Duration::from_secs(WORK_SECS - 1));
        assert_eq!(result, TickResult::Noop);
        assert_eq!(p.phase(), Phase::Working);

        let result = p.tick_at(now + Duration::from_secs(WORK_SECS));
        assert_eq!(result, TickResult::StartedBreak);
        assert_eq!(p.phase(), Phase::Break);
        assert!(p.is_input_blocked());
    }

    #[test]
    fn tick_transitions_break_to_idle_after_3min() {
        let mut p = enabled_pomodoro();
        let now = Instant::now();
        p.on_input_at(now, &[(0, 0)]);

        let break_start = now + Duration::from_secs(WORK_SECS);
        p.tick_at(break_start);
        assert_eq!(p.phase(), Phase::Break);

        p.tick_at(break_start + Duration::from_secs(BREAK_SECS - 1));
        assert_eq!(p.phase(), Phase::Break);

        p.tick_at(break_start + Duration::from_secs(BREAK_SECS));
        assert_eq!(p.phase(), Phase::Idle);
        assert!(!p.is_input_blocked());
    }

    #[test]
    fn coaching_lines_shows_analyzing_before_ai_response() {
        let mut p = enabled_pomodoro();
        let now = Instant::now();
        p.on_input_at(now, &[(0, 0)]);
        p.tick_at(now + Duration::from_secs(WORK_SECS));

        let lines = p.coaching_lines().expect("should return lines during break");
        assert_eq!(lines[0], "[Coaching]");
        assert!(lines.iter().any(|l| l.contains("분석 중")));
    }

    #[test]
    fn coaching_lines_shows_ai_response_when_available() {
        let mut p = enabled_pomodoro();
        let now = Instant::now();
        p.on_input_at(now, &[(0, 0)]);
        p.tick_at(now + Duration::from_secs(WORK_SECS));

        p.set_ai_response(vec![
            "잘 집중했습니다.".to_string(),
            "다음엔 커밋을 더 자주 하세요.".to_string(),
        ]);

        let lines = p.coaching_lines().expect("should return lines during break");
        assert_eq!(lines[0], "[Coaching]");
        assert!(lines.iter().any(|l| l.contains("잘 집중")));
        assert!(lines.iter().any(|l| l.contains("커밋")));
    }

    #[test]
    fn coaching_lines_returns_none_when_not_break() {
        let mut p = enabled_pomodoro();
        assert!(p.coaching_lines().is_none(), "Idle -> None");

        let now = Instant::now();
        p.on_input_at(now, &[(0, 0)]);
        assert_eq!(p.phase(), Phase::Working);
        assert!(p.coaching_lines().is_none(), "Working -> None");
    }

    #[test]
    fn display_text_during_working() {
        let mut p = enabled_pomodoro();
        let now = Instant::now();
        p.on_input_at(now, &[(0, 0)]);

        let text = p.display_text_at(now + Duration::from_secs(30)).unwrap();
        assert!(text.starts_with('\u{1F345}')); // 🍅
        assert!(text.contains("24:30"));
    }

    #[test]
    fn display_text_during_break() {
        let mut p = enabled_pomodoro();
        let now = Instant::now();
        p.on_input_at(now, &[(0, 0)]);

        let break_start = now + Duration::from_secs(WORK_SECS);
        p.tick_at(break_start);

        let text = p.display_text_at(break_start + Duration::from_secs(15)).unwrap();
        assert!(text.starts_with('\u{2615}')); // ☕
        assert!(text.contains("02:45"));
    }

    #[test]
    fn full_cycle_idle_work_break_idle() {
        let mut p = enabled_pomodoro();
        let now = Instant::now();

        p.on_input_at(now, &[(0, 10)]);
        assert_eq!(p.phase(), Phase::Working);

        let t1 = now + Duration::from_secs(WORK_SECS);
        p.tick_at(t1);
        assert_eq!(p.phase(), Phase::Break);

        let t2 = t1 + Duration::from_secs(BREAK_SECS);
        p.tick_at(t2);
        assert_eq!(p.phase(), Phase::Idle);

        p.on_input_at(t2 + Duration::from_secs(1), &[(0, 50)]);
        assert_eq!(p.phase(), Phase::Working);
        // New snapshot for the new cycle
        assert_eq!(p.scrollback_snapshot[&0], 50);
    }

    #[test]
    fn save_coaching_file_creates_daily_md() {
        let dir = std::env::temp_dir().join(format!("growterm_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let lines = vec![
            "좋은 집중력이었습니다.".to_string(),
            "커밋을 더 자주 해보세요.".to_string(),
        ];
        save_coaching_file(&dir, &lines);

        let entries: Vec<_> = std::fs::read_dir(&dir)
            .expect("dir should exist")
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(entries.len(), 1);

        let path = entries[0].path();
        assert!(path.extension().unwrap() == "md");

        let content = std::fs::read_to_string(&path).unwrap();
        // Should have timestamp header
        assert!(content.contains("## "), "should have timestamp header");
        assert!(content.contains("좋은 집중력"));
        assert!(content.contains("커밋"));

        // Save again — should append to same file, not create new one
        let lines2 = vec!["두 번째 코칭입니다.".to_string()];
        save_coaching_file(&dir, &lines2);

        let entries2: Vec<_> = std::fs::read_dir(&dir)
            .expect("dir should exist")
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(entries2.len(), 1, "should still be one file per day");

        let content2 = std::fs::read_to_string(&entries2[0].path()).unwrap();
        assert!(content2.contains("좋은 집중력"), "first coaching should remain");
        assert!(content2.contains("두 번째 코칭"), "second coaching should be appended");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_claude_path_returns_absolute_path_when_exists() {
        let path = find_claude_path();
        let home = std::env::var("HOME").unwrap();
        let expected = format!("{home}/.local/bin/claude");
        if std::path::Path::new(&expected).exists() {
            assert_eq!(path, expected);
            assert!(path.starts_with('/'), "should be absolute path, got: {path}");
        } else {
            assert_eq!(path, "claude");
        }
    }
}
