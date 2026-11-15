//! Child-process plumbing for yt-dlp / ffmpeg.
//!
//! Every child runs in its own process group at reduced priority, and the
//! whole group is killed on timeout, cancellation or drop — yt-dlp spawns
//! ffmpeg itself, and killing only the parent would leave ffmpeg burning CPU.

use std::ops::ControlFlow;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::{Child, Command};

/// How much of a child's stderr is kept for error reporting.
const STDERR_TAIL: usize = 8 * 1024;

pub struct Output {
    pub ok: bool,
    pub stderr: String,
}

#[derive(Debug)]
pub enum RunError {
    Spawn(std::io::Error),
    Timeout,
    /// The line callback asked to stop; the process was killed.
    Stopped,
}

/// A command with stdio closed by default, in its own process group, niced.
pub fn command(program: &str, nice: i32) -> Command {
    let mut cmd = Command::new(program);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).kill_on_drop(true);
    #[cfg(unix)]
    // SAFETY: the closure runs in the forked child before exec and only calls
    // async-signal-safe functions (setpgid, setpriority).
    unsafe {
        cmd.pre_exec(move || {
            libc::setpgid(0, 0);
            if nice > 0 {
                libc::setpriority(libc::PRIO_PROCESS as _, 0, nice);
            }
            Ok(())
        });
    }
    #[cfg(not(unix))]
    let _ = nice;
    cmd
}

fn kill_tree(child: &mut Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        // SAFETY: plain syscall; a negative pid addresses the process group we
        // created in `command`. `id()` is None once the child has been reaped,
        // so a recycled pid is never signalled.
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
    let _ = child.start_kill();
}

struct Guard {
    child: Child,
    done: bool,
}

impl Drop for Guard {
    fn drop(&mut self) {
        if !self.done {
            kill_tree(&mut self.child);
        }
    }
}

/// Run `cmd` to completion. Each stdout line (if stdout is piped) goes to
/// `on_line`, which may stop the process by returning `Break`. Only the tail
/// of stderr is retained.
pub async fn run<F>(mut cmd: Command, timeout: Duration, mut on_line: F) -> Result<Output, RunError>
where
    F: FnMut(&str) -> ControlFlow<()> + Send,
{
    let child = cmd.spawn().map_err(RunError::Spawn)?;
    let mut guard = Guard { child, done: false };
    let stdout = guard.child.stdout.take();
    let stderr = guard.child.stderr.take();

    let stderr_task = tokio::spawn(async move {
        let mut tail: Vec<u8> = Vec::new();
        if let Some(mut stderr) = stderr {
            let mut chunk = [0u8; 2048];
            while let Ok(n) = stderr.read(&mut chunk).await {
                if n == 0 {
                    break;
                }
                tail.extend_from_slice(&chunk[..n]);
                if tail.len() > 2 * STDERR_TAIL {
                    let cut = tail.len() - STDERR_TAIL;
                    tail.drain(..cut);
                }
            }
        }
        String::from_utf8_lossy(&tail).into_owned()
    });

    let body = async {
        let mut stopped = false;
        if let Some(stdout) = stdout {
            let mut reader = BufReader::new(stdout);
            let mut buf = Vec::with_capacity(256);
            loop {
                buf.clear();
                match reader.read_until(b'\n', &mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                if on_line(String::from_utf8_lossy(&buf).trim_end()).is_break() {
                    stopped = true;
                    break;
                }
            }
        }
        if stopped {
            kill_tree(&mut guard.child);
        }
        let status = guard.child.wait().await;
        guard.done = true;
        (stopped, status)
    };

    match tokio::time::timeout(timeout, body).await {
        // `guard` is dropped on return and kills the process group.
        Err(_) => Err(RunError::Timeout),
        Ok((true, _)) => Err(RunError::Stopped),
        Ok((false, status)) => {
            let status = status.map_err(RunError::Spawn)?;
            let stderr = stderr_task.await.unwrap_or_default();
            Ok(Output { ok: status.success(), stderr })
        }
    }
}

/// Run a short command and capture its stdout (bounded) as text.
pub async fn capture(mut cmd: Command, timeout: Duration) -> Result<(Output, String), RunError> {
    const MAX_STDOUT: usize = 256 * 1024;
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut out = String::new();
    let result = run(cmd, timeout, |line| {
        if out.len() + line.len() < MAX_STDOUT {
            out.push_str(line);
            out.push('\n');
        }
        ControlFlow::Continue(())
    })
    .await?;
    Ok((result, out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(script: &str) -> Command {
        let mut cmd = if cfg!(windows) {
            let mut c = command("cmd", 0);
            c.args(["/C", script]);
            c
        } else {
            let mut c = command("sh", 5);
            c.args(["-c", script]);
            c
        };
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        cmd
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
    }

    #[test]
    fn captures_lines_and_status() {
        rt().block_on(async {
            let (out, text) = capture(shell("echo one&& echo two"), Duration::from_secs(20)).await.unwrap();
            assert!(out.ok);
            assert_eq!(text.lines().map(str::trim).collect::<Vec<_>>(), vec!["one", "two"]);

            let (out, _) = capture(shell("echo oops 1>&2&& exit 3"), Duration::from_secs(20)).await.unwrap();
            assert!(!out.ok);
            assert!(out.stderr.contains("oops"));
        });
    }

    #[test]
    fn stops_and_times_out() {
        rt().block_on(async {
            let sleep = if cfg!(windows) { "echo x&& ping -n 30 127.0.0.1 >NUL" } else { "echo x; sleep 30" };
            let started = std::time::Instant::now();
            let res = run(shell(sleep), Duration::from_secs(20), |_| ControlFlow::Break(())).await;
            assert!(matches!(res, Err(RunError::Stopped)));

            let res = run(shell(sleep), Duration::from_millis(300), |_| ControlFlow::Continue(())).await;
            assert!(matches!(res, Err(RunError::Timeout)));
            assert!(started.elapsed() < Duration::from_secs(15));

            let res =
                run(command("definitely-not-a-real-binary", 0), Duration::from_secs(5), |_| ControlFlow::Continue(()))
                    .await;
            assert!(matches!(res, Err(RunError::Spawn(_))));
        });
    }
}
