//! macOS LaunchAgent install/uninstall for quickshot autostart.
//!
//! On non-macOS platforms the functions return an error — the plan
//! explicitly targets macOS for Iter 3.

use anyhow::{bail, Context, Result};
#[cfg(target_os = "macos")]
use std::path::PathBuf;

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
    let status = std::process::Command::new("launchctl")
        .arg("load")
        .arg(&plist_path)
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => eprintln!(
            "autostart: launchctl load exited with {s}; plist installed but not loaded"
        ),
        Err(e) => eprintln!("autostart: launchctl not runnable: {e}"),
    }
    println!("installed autostart → {}", plist_path.display());
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn uninstall() -> Result<()> {
    let plist_path = plist_path()?;
    let _ = std::process::Command::new("launchctl")
        .arg("unload")
        .arg(&plist_path)
        .status();
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

#[cfg(not(target_os = "macos"))]
pub fn install() -> Result<()> {
    bail!("autostart is only supported on macOS");
}

#[cfg(not(target_os = "macos"))]
pub fn uninstall() -> Result<()> {
    bail!("autostart is only supported on macOS");
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
