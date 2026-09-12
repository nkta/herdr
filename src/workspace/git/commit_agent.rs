use std::io::{Read, Write};
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

/// Diff text past this size is truncated before it reaches the agent prompt, so a huge staged
/// change can't balloon the subprocess invocation or the agent's own context.
const MAX_DIFF_PROMPT_BYTES: usize = 200 * 1024;
/// Caps how much of the agent's stdout/stderr is read, so a runaway or misbehaving agent process
/// can't grow memory unbounded while Herdr waits on it.
const MAX_AGENT_OUTPUT_BYTES: usize = 16 * 1024;

/// Builds the prompt sent to a commit-message-generation agent: a fixed instruction plus the
/// staged diff.
pub fn build_commit_message_prompt(diff: &str) -> String {
    let truncated = if diff.len() > MAX_DIFF_PROMPT_BYTES {
        format!("{}\n... [diff truncated]", &diff[..MAX_DIFF_PROMPT_BYTES])
    } else {
        diff.to_string()
    };
    format!(
        "Write a concise git commit message (a short summary line, optionally a blank line then \
         a brief body) describing the following staged changes. Reply with only the commit \
         message text: no markdown fences, no preamble, no explanation. Do not add a \
         signature, co-author line, or any attribution or mention of an AI, assistant, or \
         coding agent having generated this message or the change.\n\n{truncated}"
    )
}

/// Runs `command args...` with `prompt` written to its stdin and captures stdout as the
/// generated commit message. The process is killed if it outlives `timeout`.
pub fn generate_commit_message(
    repo_root: &Path,
    command: &str,
    args: &[String],
    prompt: &str,
    timeout: Duration,
) -> Result<String, String> {
    let mut cmd = crate::noninteractive_process::command(command);
    cmd.args(args)
        .current_dir(repo_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|err| format!("failed to start \"{command}\": {err}"))?;

    let stdin = match child.stdin.take() {
        Some(stdin) => stdin,
        None => return Err(format!("failed to open stdin for \"{command}\"")),
    };
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => return Err(format!("failed to open stdout for \"{command}\"")),
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => return Err(format!("failed to open stderr for \"{command}\"")),
    };

    let prompt_owned = prompt.to_string();
    let writer = std::thread::spawn(move || {
        let mut stdin = stdin;
        let _ = stdin.write_all(prompt_owned.as_bytes());
        // `stdin` drops here, closing the pipe so the child sees EOF.
    });
    let stdout_reader = std::thread::spawn(move || read_capped(stdout, MAX_AGENT_OUTPUT_BYTES));
    let stderr_reader = std::thread::spawn(move || read_capped(stderr, MAX_AGENT_OUTPUT_BYTES));

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!(
                    "\"{command}\" timed out after {}s",
                    timeout.as_secs()
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(err) => break Err(err.to_string()),
        }
    };
    let _ = writer.join();
    let stdout_text = stdout_reader.join().unwrap_or_default();
    let stderr_text = stderr_reader.join().unwrap_or_default();

    let status = status?;
    if !status.success() {
        return Err(format!(
            "\"{command}\" exited with {status}: {}",
            stderr_text.trim()
        ));
    }
    let message = stdout_text.trim().to_string();
    if message.is_empty() {
        return Err(format!("\"{command}\" produced no output"));
    }
    Ok(message)
}

fn read_capped(mut reader: impl Read, max_bytes: usize) -> String {
    let mut buf = Vec::new();
    let _ = reader.by_ref().take(max_bytes as u64).read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::git::test_support::temp_test_dir;

    #[cfg(unix)]
    fn echo_command() -> (&'static str, Vec<String>) {
        ("sh", vec!["-c".to_string(), "cat".to_string()])
    }

    #[cfg(windows)]
    fn echo_command() -> (&'static str, Vec<String>) {
        ("cmd", vec!["/C".to_string(), "more".to_string()])
    }

    #[test]
    fn generates_message_from_stdin_to_stdout() {
        let repo = temp_test_dir("commit-agent-echo");
        let (command, args) = echo_command();

        let result =
            generate_commit_message(&repo, command, &args, "fix bug\n", Duration::from_secs(5));

        assert_eq!(result.unwrap(), "fix bug");
        std::fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn missing_binary_produces_a_clear_error() {
        let repo = temp_test_dir("commit-agent-missing");

        let result = generate_commit_message(
            &repo,
            "herdr-commit-agent-does-not-exist",
            &[],
            "prompt",
            Duration::from_secs(5),
        );

        assert!(result.is_err());
        std::fs::remove_dir_all(repo).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn timeout_kills_the_process() {
        let repo = temp_test_dir("commit-agent-timeout");

        let result = generate_commit_message(
            &repo,
            "sh",
            &["-c".to_string(), "sleep 5".to_string()],
            "prompt",
            Duration::from_millis(100),
        );

        let err = result.unwrap_err();
        assert!(err.contains("timed out"), "unexpected error: {err}");
        std::fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn prompt_includes_the_diff_and_truncates_when_huge() {
        let prompt = build_commit_message_prompt("diff --git a/f b/f\n+hello\n");
        assert!(prompt.contains("diff --git a/f b/f"));
        assert!(prompt.contains("+hello"));

        let huge_diff = "x".repeat(MAX_DIFF_PROMPT_BYTES * 2);
        let truncated_prompt = build_commit_message_prompt(&huge_diff);
        assert!(truncated_prompt.contains("[diff truncated]"));
        assert!(truncated_prompt.len() < huge_diff.len());
    }
}
