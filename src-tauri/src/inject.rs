use anyhow::{Context, Result};
use arboard::Clipboard;
use enigo::{Direction, Enigo, Key, Keyboard, Settings as EnigoSettings};
use std::thread::sleep;
use std::time::Duration;

/// Types `text` into whatever input currently has focus using the
/// clipboard + Cmd+V pattern. Direct AX insertion is unreliable across
/// Electron apps, web views and terminals — synthesized paste works everywhere.
/// The previous clipboard contents are restored afterwards.
pub fn inject_text(text: &str) -> Result<()> {
    if text.is_empty() {
        return Ok(());
    }

    let mut clipboard = Clipboard::new().context("failed to open clipboard")?;
    let previous = clipboard.get_text().ok();

    clipboard
        .set_text(text.to_string())
        .context("failed to write clipboard")?;

    // Give the pasteboard a beat to settle before synthesizing the keystroke.
    sleep(Duration::from_millis(50));

    let mut enigo =
        Enigo::new(&EnigoSettings::default()).context("failed to init key synthesizer")?;
    enigo.key(Key::Meta, Direction::Press)?;
    enigo.key(Key::Unicode('v'), Direction::Click)?;
    enigo.key(Key::Meta, Direction::Release)?;

    // Let the target app consume the paste before the clipboard is restored.
    sleep(Duration::from_millis(300));
    if let Some(previous) = previous {
        let _ = clipboard.set_text(previous);
    }
    Ok(())
}

/// Synthesizing keystrokes requires the Accessibility permission. This checks
/// it and, when missing, makes macOS show the system prompt pointing the user
/// to System Settings > Privacy & Security > Accessibility.
pub fn ensure_accessibility() -> bool {
    macos_accessibility_client::accessibility::application_is_trusted_with_prompt()
}
