//! Per-user autostart install/uninstall for quickshot.
//!
//! - macOS: writes a `~/Library/LaunchAgents/com.quickshot.daemon.plist`.
//! - Windows: writes a string value under
//!   `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` via `reg.exe`.
//! - Other Unixes: not supported; functions return an error.

#[cfg(any(target_os = "macos", target_os = "windows"))]
use anyhow::Context;
use anyhow::{bail, Result};
#[cfg(target_os = "macos")]
use std::path::PathBuf;

#[allow(dead_code)]
const LABEL: &str = "com.quickshot.daemon";

#[cfg(target_os = "macos")]
pub fn install() -> Result<()> {
    let bin = std::env::current_exe().context("resolve current executable path")?;
    let bin_str = bin.to_string_lossy().into_owned();
    let plist = render_plist(&bin_str);
    let plist_path = plist_path()?;
    if let Some(parent) = plist_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!("create LaunchAgents dir {}", parent.display())
        })?;
    }
    std::fs::write(&plist_path, plist).with_context(|| {
        format!("write plist to {}", plist_path.display())
    })?;
    // Intentionally NOT calling `launchctl load`. launchd scans
    // ~/Library/LaunchAgents/*.plist at login, so the plist is picked up
    // automatically on next login. Calling `launchctl load` now would honor
    // RunAtLoad=true immediately and spawn a *second* quickshot instance
    // alongside the process currently running (the one showing the menu).
    println!("installed autostart → {}", plist_path.display());
    println!("(takes effect at next login)");
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn uninstall() -> Result<()> {
    let plist_path = plist_path()?;
    // Intentionally NOT calling `launchctl unload`. If the process currently
    // handling this menu click was itself launched by launchd (user enabled
    // autostart, logged out, logged in, then clicked disable), `launchctl
    // unload` SIGTERMs that process. The app appears to exit instantly.
    // Just removing the plist is sufficient: launchd won't see it at next
    // login, and the current session keeps running until manual Quit.
    match std::fs::remove_file(&plist_path) {
        Ok(()) => println!("removed autostart"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("removed autostart (nothing to remove)");
        }
        Err(e) => {
            bail!("failed to remove {}: {e}", plist_path.display());
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn is_installed() -> bool {
    match plist_path() {
        Ok(p) => p.exists(),
        Err(_) => false,
    }
}

// --- Windows -------------------------------------------------------------

#[cfg(target_os = "windows")]
const WIN_RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(target_os = "windows")]
const WIN_VALUE_NAME: &str = "quickshot";
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[cfg(target_os = "windows")]
fn reg_cmd() -> std::process::Command {
    use std::os::windows::process::CommandExt;
    let mut c = std::process::Command::new("reg.exe");
    // Suppress the brief console flash that would otherwise pop up when our
    // GUI-subsystem process spawns a console child.
    c.creation_flags(CREATE_NO_WINDOW);
    c
}

#[cfg(target_os = "windows")]
pub fn install() -> Result<()> {
    let bin = std::env::current_exe().context("resolve current executable path")?;
    let bin_str = bin.to_string_lossy().into_owned();
    // Quote the path so spaces in the install location survive the
    // CreateProcess invocation that Windows uses when reading Run values.
    let value = format!("\"{bin_str}\"");
    let output = reg_cmd()
        .args([
            "add",
            WIN_RUN_KEY,
            "/v",
            WIN_VALUE_NAME,
            "/t",
            "REG_SZ",
            "/d",
            &value,
            "/f",
        ])
        .output()
        .context("invoke reg.exe add")?;
    if !output.status.success() {
        bail!(
            "reg add failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    println!("installed autostart → {WIN_RUN_KEY}\\{WIN_VALUE_NAME} = {value}");
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn uninstall() -> Result<()> {
    let output = reg_cmd()
        .args(["delete", WIN_RUN_KEY, "/v", WIN_VALUE_NAME, "/f"])
        .output()
        .context("invoke reg.exe delete")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // reg.exe exits non-zero if the value doesn't exist; treat as success.
        if stderr.to_ascii_lowercase().contains("unable to find")
            || stderr.contains("ERROR: The system was unable")
        {
            println!("removed autostart (nothing to remove)");
            return Ok(());
        }
        bail!("reg delete failed: {}", stderr.trim());
    }
    println!("removed autostart");
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn is_installed() -> bool {
    let output = reg_cmd()
        .args(["query", WIN_RUN_KEY, "/v", WIN_VALUE_NAME])
        .output();
    match output {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}

// --- Unsupported platforms ----------------------------------------------

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn is_installed() -> bool {
    false
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn install() -> Result<()> {
    bail!("autostart is not supported on this platform");
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn uninstall() -> Result<()> {
    bail!("autostart is not supported on this platform");
}

#[cfg(target_os = "macos")]
fn plist_path() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME env var not set")?;
    Ok(PathBuf::from(home)
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{LABEL}.plist")))
}

/// Render the plist with `bin` substituted in. Exposed for unit testing.
pub fn render_plist(bin: &str) -> String {
    let escaped = escape_xml(bin);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{escaped}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <false/>
    <key>StandardErrorPath</key>
    <string>/tmp/quickshot.stderr.log</string>
    <key>StandardOutPath</key>
    <string>/tmp/quickshot.stdout.log</string>
</dict>
</plist>
"#
    )
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_contains_bin_path() {
        let out = render_plist("/Users/test/quickshot");
        assert!(out.contains("<string>/Users/test/quickshot</string>"));
        assert!(out.contains("<key>Label</key>"));
        assert!(out.contains("com.quickshot.daemon"));
        assert!(out.contains("<key>RunAtLoad</key>"));
        assert!(out.contains("<true/>"));
        assert!(out.contains("<key>KeepAlive</key>"));
        // The <true/> assertion above matches RunAtLoad's <true/>; verify <false/> also present for KeepAlive.
        assert!(out.contains("<false/>"));
    }

    #[test]
    fn plist_escapes_xml_chars() {
        let out = render_plist("/home/user/dir with <brackets>&ampersands/bin");
        assert!(out.contains("&lt;brackets&gt;"));
        assert!(out.contains("&amp;ampersands"));
        assert!(!out.contains("<brackets>"));
    }

    #[test]
    fn escape_xml_cases() {
        assert_eq!(escape_xml("a&b"), "a&amp;b");
        assert_eq!(escape_xml("a<b>c"), "a&lt;b&gt;c");
        assert_eq!(escape_xml("plain/path"), "plain/path");
    }
}
