//! Runs a local LLM CLI (claude, codex, gemini, ollama, ...) as a subprocess.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use super::ProviderConfig;

pub fn is_available(provider: &ProviderConfig) -> bool {
    which::which(&provider.command).is_ok()
}

/// `model`: the model to pass to the tool, `None` for the tool's default.
pub fn run(
    provider: &ProviderConfig,
    model: Option<&str>,
    prompt: &str,
    timeout: Duration,
    cancel: Arc<AtomicBool>,
) -> Result<String> {
    // A private scratch dir: agentic CLIs start there instead of whatever cwd we were launched from.
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let work_dir = std::env::temp_dir().join(format!(
        "memento-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&work_dir)?;
    let _cleanup = RemoveOnDrop(work_dir.clone());

    let prompt_file = work_dir.join("prompt.md");
    let output_file = work_dir.join("output.txt");
    std::fs::write(&prompt_file, prompt)?;

    let args = provider.command_args(model);
    let uses_output_file = args.iter().any(|a| a.contains("{output_file}"));
    let args: Vec<String> = args
        .iter()
        .map(|a| {
            a.replace("{prompt_file}", &prompt_file.to_string_lossy())
                .replace("{output_file}", &output_file.to_string_lossy())
        })
        .collect();

    let exe = which::which(&provider.command)
        .with_context(|| format!("`{}` not found on PATH", provider.command))?;
    let mut child = Command::new(exe)
        .args(&args)
        .current_dir(&work_dir)
        .stdin(if provider.stdin {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to start {}", provider.label(model)))?;

    if let Some(mut stdin) = child.stdin.take() {
        let prompt = prompt.to_owned();
        std::thread::spawn(move || {
            let _ = stdin.write_all(prompt.as_bytes());
        });
    }
    let stdout = read_in_background(child.stdout.take());
    let stderr = read_in_background(child.stderr.take());

    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("cancelled");
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            bail!("{} timed out after {}s", provider.name, timeout.as_secs());
        }
        std::thread::sleep(Duration::from_millis(200));
    };

    let stdout = stdout.join().unwrap_or_default();
    let stderr = stderr.join().unwrap_or_default();
    if !status.success() {
        bail!(
            "{} exited with {}: {}",
            provider.name,
            status,
            last_lines(if stderr.trim().is_empty() { &stdout } else { &stderr }, 15)
        );
    }

    let output = if uses_output_file {
        std::fs::read_to_string(&output_file).unwrap_or(stdout)
    } else {
        stdout
    };
    if output.trim().is_empty() {
        bail!("{} returned an empty response. stderr: {}", provider.name, last_lines(&stderr, 15));
    }
    Ok(output)
}

fn read_in_background<R: Read + Send + 'static>(
    pipe: Option<R>,
) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut p) = pipe {
            let _ = p.read_to_end(&mut buf);
        }
        String::from_utf8_lossy(&buf).into_owned()
    })
}

fn last_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.trim().lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

struct RemoveOnDrop(PathBuf);
impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
