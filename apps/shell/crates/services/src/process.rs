use std::{
    env,
    ffi::OsStr,
    fs,
    io::Write as _,
    path::PathBuf,
    process::{Command, Stdio},
};

pub(crate) fn run(program: &str, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(arguments)
        .env("LC_ALL", "C")
        .output()
        .map_err(|error| short_error(program, &error))?;
    output_text(program, output)
}

pub(crate) fn spawn_detached<I, S>(program: &str, arguments: I) -> Result<(), String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| short_error(program, &error))
}

pub(crate) fn run_with_stdin(
    program: &str,
    arguments: &[&str],
    input: &str,
) -> Result<String, String> {
    let mut child = Command::new(program)
        .args(arguments)
        .env("LC_ALL", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| short_error(program, &error))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(input.as_bytes())
            .map_err(|error| short_error(program, &error))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|error| short_error(program, &error))?;
    output_text(program, output)
}

fn output_text(program: &str, output: std::process::Output) -> Result<String, String> {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = stderr
            .lines()
            .chain(stdout.lines())
            .find(|line| !line.trim().is_empty())
            .unwrap_or("command failed")
            .trim();
        return Err(format!("{program}: {detail}"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().into())
}

pub(crate) fn executable_in_path(name: &str) -> bool {
    env::var_os("PATH").is_some_and(|path| {
        env::split_paths(&path).any(|directory| is_executable(directory.join(name)))
    })
}

fn is_executable(path: PathBuf) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

pub(crate) fn short_error(subject: &str, error: &impl std::fmt::Display) -> String {
    format!("{subject}: {error}")
}
