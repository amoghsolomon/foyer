use std::{
    io::Write as _,
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use anyhow::{Context as _, Result, bail};

pub fn copy_and_paste(text: &str) -> Result<()> {
    if text.trim().is_empty() {
        bail!("refusing to paste an empty transcript");
    }

    let mut clipboard = Command::new("wl-copy")
        .args(["--trim-newline", "--type", "text/plain;charset=utf-8"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("start wl-copy")?;
    clipboard
        .stdin
        .take()
        .context("open wl-copy input")?
        .write_all(text.as_bytes())
        .context("write transcript to the clipboard")?;
    let output = clipboard.wait_with_output().context("wait for wl-copy")?;
    if !output.status.success() {
        bail!(
            "wl-copy failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    thread::sleep(Duration::from_millis(60));
    let output = Command::new("ydotool")
        .args(["key", "42:1", "110:1", "110:0", "42:0"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .context("start ydotool")?;
    if !output.status.success() {
        bail!(
            "ydotool could not send Shift+Insert: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}
