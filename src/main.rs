mod app;
mod capture;
mod clipboard;
mod crop;
mod hotkey;
mod overlay;
mod permission;

fn main() -> anyhow::Result<()> {
    permission::preflight()?;
    println!("quickshot starting; press Ctrl/Cmd+Shift+A to capture.");
    Ok(())
}
