use std::process::Command;
use std::time::Duration;

use growterm_integration_tests::{
    build_binary, cleanup, has_claude_cli, parse_dump_rows, spawn_with_dump_and_input, wait_for_dump,
};

#[test]
fn launches_claude_code_in_shell() {
    if std::env::var("GROWTERM_RUN_CLAUDE_CODE_TEST").ok().as_deref() != Some("1") {
        eprintln!("skip: set GROWTERM_RUN_CLAUDE_CODE_TEST=1 to run Claude Code launch test");
        return;
    }

    if !has_claude_cli() {
        eprintln!("skip: claude CLI is not installed or not in PATH");
        return;
    }

    let bin = build_binary();
    let dump_path = std::env::temp_dir().join(format!(
        "growterm_claude_launch_{}.txt",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&dump_path);

    let mut child = spawn_with_dump_and_input(&bin, &dump_path, "env -u CLAUDECODE -u CLAUDE_CODE_ENTRYPOINT claude\n");

    // Wait longer: claude needs time to start up after env unset
    std::thread::sleep(Duration::from_secs(5));
    let _ = std::fs::remove_file(&dump_path);
    let dump_content = wait_for_dump(&dump_path, Duration::from_secs(30), Some(&mut child));

    if dump_content.is_none() {
        let status = child.try_wait().ok().flatten();
        let stderr = {
            use std::io::Read;
            let mut buf = String::new();
            if let Some(err) = child.stderr.as_mut() {
                let _ = err.read_to_string(&mut buf);
            }
            buf
        };
        cleanup(&mut child, &dump_path);
        panic!(
            "grid dump was not created while launching claude. child_status={status:?}\nstderr:\n{stderr}"
        );
    }

    let content = dump_content.expect("dump already checked");
    let rows = parse_dump_rows(&content);
    let all_text = rows.join("\n");

    cleanup(&mut child, &dump_path);

    assert!(
        all_text.contains("claude") || all_text.contains("Claude"),
        "expected dump to contain claude launch trace, got:\n{all_text}"
    );
}
