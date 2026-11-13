use anyhow::Result;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct CaptureNotice {
    dimensions: String,
    pub saved: Option<std::path::PathBuf>,
}

impl CaptureNotice {
    pub fn new(width: u32, height: u32, saved: Option<std::path::PathBuf>) -> Self {
        Self {
            dimensions: format!("{width} × {height}"),
            saved,
        }
    }

    pub fn busy(width: u32, height: u32) -> Self {
        Self {
            dimensions: format!("{width} × {height} · 保存繁忙，仅复制到剪贴板"),
            saved: None,
        }
    }

    pub fn show(&self) -> Result<()> {
        let path = self.saved.as_deref().map(std::path::absolute).transpose()?;
        show(&self.dimensions, path.as_deref())
    }
}

pub(crate) fn body(dimensions: &str, saved: Option<&Path>) -> String {
    match saved {
        Some(_) => format!("{dimensions} · 点击查看"),
        None => dimensions.to_owned(),
    }
}

#[cfg(windows)]
use crate::windows_notification::show;

#[cfg(target_os = "macos")]
fn show(dimensions: &str, path: Option<&Path>) -> Result<()> {
    use std::cell::RefCell;
    use std::io::Write;
    use std::process::{Child, Command, Stdio};

    let exe = std::env::current_exe()?;
    let contents = exe
        .parent()
        .and_then(Path::parent)
        .filter(|p| p.ends_with("Contents"));
    let Some(contents) = contents else {
        return plain_notification(dimensions, path);
    };
    let helper =
        contents.join("Helpers/QuickShot Notifications.app/Contents/MacOS/QuickShotNotifications");
    let mut message = serde_json::to_vec(&serde_json::json!({
        "body": body(dimensions, path),
        "path": path,
    }))?;
    message.push(b'\n');

    // Fullscreen notifications are delivered on the event-loop thread. A single
    // pipe preserves capture order and closes automatically when QuickShot exits.
    thread_local! { static HELPER: RefCell<Option<Child>> = const { RefCell::new(None) }; }
    HELPER.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some(child) = slot.as_mut() {
            if child.try_wait()?.is_none()
                && child.stdin.as_mut().unwrap().write_all(&message).is_ok()
            {
                return Ok(());
            }
            drop(child.stdin.take());
            child.wait()?;
        }
        let mut child = Command::new(helper)
            .arg("--listen")
            .stdin(Stdio::piped())
            .spawn()?;
        let result = child.stdin.as_mut().unwrap().write_all(&message);
        *slot = Some(child);
        Ok(result?)
    })
}

#[cfg(not(any(windows, target_os = "macos")))]
fn show(dimensions: &str, path: Option<&Path>) -> Result<()> {
    plain_notification(dimensions, path)
}

#[cfg(not(windows))]
fn plain_notification(body: &str, path: Option<&Path>) -> Result<()> {
    notify_rust::Notification::new()
        .appname("QuickShot")
        .summary(if path.is_some() {
            "Screenshot saved"
        } else {
            "Screenshot copied"
        })
        .body(body)
        .show()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offer_reveal_only_for_a_saved_file() {
        assert_eq!(body("2 × 3", None), "2 × 3");
        assert_eq!(
            body("2 × 3", Some(Path::new("shot.png"))),
            "2 × 3 · 点击查看"
        );
    }
}
