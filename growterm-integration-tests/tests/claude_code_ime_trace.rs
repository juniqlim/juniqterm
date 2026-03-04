use std::process::Command;
use std::time::Duration;

use growterm_integration_tests::{
    activate_by_pid, build_binary, cleanup, has_claude_cli, korean_input_source_selected,
    parse_dump, spawn_with_dump_and_input, wait_for_dump,
};

fn debug_mode() -> bool {
    std::env::var("GROWTERM_IME_TRACE_DEBUG").ok().as_deref() == Some("1")
}

fn send_keystroke(s: &str) {
    let script = format!("tell application \"System Events\" to keystroke \"{s}\"");
    let _ = Command::new("osascript").arg("-e").arg(script).output();
}

fn find_char_positions(rows: &[String], target: char) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for (r, row) in rows.iter().enumerate() {
        for (c, ch) in row.chars().enumerate() {
            if ch == target {
                out.push((r, c));
            }
        }
    }
    out
}

#[test]
fn claude_code_hangul_composition_cursor_matches_visible_char_position() {
    if std::env::var("GROWTERM_RUN_CLAUDE_CODE_IME_TRACE_TEST")
        .ok()
        .as_deref()
        != Some("1")
    {
        eprintln!(
            "skip: set GROWTERM_RUN_CLAUDE_CODE_IME_TRACE_TEST=1 to run Claude Code IME trace test"
        );
        return;
    }

    if !has_claude_cli() {
        eprintln!("skip: claude CLI is not installed or not in PATH");
        return;
    }

    if !korean_input_source_selected() {
        eprintln!("skip: Korean input source is not selected");
        return;
    }

    let bin = build_binary();
    let dump_path = std::env::temp_dir().join(format!(
        "growterm_claude_ime_trace_{}.txt",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&dump_path);

    let mut child = spawn_with_dump_and_input(&bin, &dump_path, "env -u CLAUDECODE -u CLAUDE_CODE_ENTRYPOINT claude\n");

    let initial = wait_for_dump(&dump_path, Duration::from_secs(15), Some(&mut child));
    assert!(
        initial.is_some(),
        "failed to receive initial dump before IME input"
    );

    // // Wait for claude to fully start before sending keystrokes
    // std::thread::sleep(Duration::from_millis(500));

    if debug_mode() {
        activate_by_pid(child.id());
    }

    let steps: [(char, char); 3] = [('ㅎ', 'ㅎ'), ('ㅏ', '하'), ('ㄴ', '한')];

    let mut failures: Vec<String> = Vec::new();

    for (input, expected_visible_char) in steps {
        let _ = std::fs::remove_file(&dump_path);
        send_keystroke(&input.to_string());

        let Some(snapshot) = wait_for_dump(&dump_path, Duration::from_secs(10), Some(&mut child))
        else {
            failures.push(format!("no dump after input step '{input}'"));
            break;
        };

        let (cursor_row, cursor_col, rows) = parse_dump(&snapshot);
        let positions = find_char_positions(&rows, expected_visible_char);

        if debug_mode() {
            eprintln!(
                "step input='{input}' expect='{expected_visible_char}' cursor=({cursor_row},{cursor_col}) positions={positions:?}"
            );
        }

        if positions.is_empty() {
            failures.push(format!(
                "expected visible char '{expected_visible_char}' after input '{input}', but not found in grid"
            ));
            continue;
        }

        let cursor = (cursor_row as usize, cursor_col as usize);
        if !positions.contains(&cursor) {
            failures.push(format!(
                "input '{input}' expected cursor at visible char '{expected_visible_char}' position, cursor={cursor:?}, positions={positions:?}"
            ));
        }
    }

    cleanup(&mut child, &dump_path);

    assert!(
        failures.is_empty(),
        "cursor/IME position mismatch detected:\n{}",
        failures.join("\n")
    );
}
