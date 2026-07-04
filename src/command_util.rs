use anyhow::{Context, Result, bail};
use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::thread;

pub fn command_exists(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };

    std::env::split_paths(&path).any(|dir| {
        let candidate = dir.join(name);
        candidate.is_file()
    })
}

pub fn run_capture<I, S>(program: &str, args: I) -> Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("failed to execute `{program}`"))?;

    if !output.status.success() {
        bail!(
            "`{program}` exited with status {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(output)
}

pub fn run_streaming<I, S>(program: &str, args: I, cwd: Option<&Path>) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new(program);
    command.args(args);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let status = command
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("failed to execute `{program}`"))?;

    if !status.success() {
        bail!("`{program}` exited with status {status}");
    }

    Ok(())
}

pub fn spawn_with_log_streaming(
    mut command: Command,
    log_path: &Path,
) -> Result<std::process::Child> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to spawn child process")?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let log_path_stdout = log_path.to_path_buf();
    let log_path_stderr = log_path.to_path_buf();

    if let Some(stdout) = stdout {
        thread::spawn(move || {
            let _ = stream_reader(stdout, &log_path_stdout, false);
        });
    }

    if let Some(stderr) = stderr {
        thread::spawn(move || {
            let _ = stream_reader(stderr, &log_path_stderr, true);
        });
    }

    Ok(child)
}

fn stream_reader<R: Read>(reader: R, log_path: &Path, stderr: bool) -> Result<()> {
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .with_context(|| format!("failed to open log file {}", log_path.display()))?;

    for line in BufReader::new(reader).lines() {
        let line = line?;
        if stderr {
            eprintln!("{line}");
        } else {
            println!("{line}");
        }
        writeln!(log, "{line}")?;
    }

    Ok(())
}

pub fn platform_is_linux() -> bool {
    std::env::consts::OS == "linux"
}
