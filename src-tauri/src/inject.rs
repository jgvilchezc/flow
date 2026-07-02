use anyhow::{Context, Result};
use arboard::Clipboard;
use enigo::{Direction, Enigo, Key, Keyboard, Settings as EnigoSettings};
use objc2_app_kit::NSPasteboard;
use std::thread::sleep;
use std::time::{Duration, Instant};

/// Upper bound on the confirmed pre-paste wait. The pasteboard write almost
/// always lands in a millisecond or two; this caps the loop so a stuck
/// `changeCount` can never wedge the paste.
const PRE_PASTE_POLL_CAP_MS: u64 = 60;
/// Sleep granularity while waiting for the pasteboard write to be confirmed.
const POLL_INTERVAL_MS: u64 = 5;
/// Fixed wait after synthesizing Cmd+V before restoring the clipboard. Paste
/// consumption emits no OS signal, so this bounded sleep is the only safe knob;
/// too short and the target app pastes the restored (previous) contents.
const POST_PASTE_MS: u64 = 120;

/// Returns whether the pre-paste confirmation loop should keep waiting.
///
/// Stops immediately once the pasteboard `changeCount` has moved (`changed`),
/// and never waits past [`PRE_PASTE_POLL_CAP_MS`]. Kept pure so the timing
/// contract is unit-testable without touching AppKit.
fn should_continue_polling(elapsed_ms: u64, changed: bool) -> bool {
    !changed && elapsed_ms < PRE_PASTE_POLL_CAP_MS
}

/// Types `text` into whatever input currently has focus using the
/// clipboard + Cmd+V pattern. Direct AX insertion is unreliable across
/// Electron apps, web views and terminals — synthesized paste works everywhere.
/// The previous clipboard contents are restored afterwards.
///
/// Must run on the main thread (AppKit requirement for `NSPasteboard` and the
/// enigo key synthesizer); the caller dispatches it via `run_on_main_thread`.
pub fn inject_text(text: &str) -> Result<()> {
    if text.is_empty() {
        return Ok(());
    }

    let mut clipboard = Clipboard::new().context("failed to open clipboard")?;
    let previous = clipboard.get_text().ok();

    // Snapshot the pasteboard change counter BEFORE writing so the write can be
    // confirmed by an increment rather than a blind fixed sleep.
    let pasteboard = NSPasteboard::generalPasteboard();
    let before_count = pasteboard.changeCount();

    clipboard
        .set_text(text.to_string())
        .context("failed to write clipboard")?;

    // Poll until the pasteboard write is confirmed (changeCount moved) or the
    // cap elapses, then paste regardless. Usually this returns in a couple of
    // milliseconds — far faster than the old fixed 50ms settle.
    let poll_started = Instant::now();
    loop {
        let elapsed = poll_started.elapsed().as_millis() as u64;
        let changed = pasteboard.changeCount() != before_count;
        if !should_continue_polling(elapsed, changed) {
            break;
        }
        sleep(Duration::from_millis(POLL_INTERVAL_MS));
    }

    let mut enigo =
        Enigo::new(&EnigoSettings::default()).context("failed to init key synthesizer")?;
    enigo.key(Key::Meta, Direction::Press)?;
    enigo.key(Key::Unicode('v'), Direction::Click)?;
    enigo.key(Key::Meta, Direction::Release)?;

    // Let the target app consume the paste before the clipboard is restored.
    // No signal marks consumption, so this stays a bounded fixed sleep.
    sleep(Duration::from_millis(POST_PASTE_MS));
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

/// Passive permission check — never triggers the system prompt. UI status
/// must use this; the prompting variant is reserved for explicit user action
/// (the grant button, or the first dictation attempt).
pub fn is_accessibility_granted() -> bool {
    macos_accessibility_client::accessibility::application_is_trusted()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polling_stops_once_change_is_observed() {
        // A confirmed pasteboard write ends the wait immediately, even at 0ms.
        assert!(!should_continue_polling(0, true));
        assert!(!should_continue_polling(30, true));
    }

    #[test]
    fn polling_stops_at_or_after_the_cap() {
        // The cap is exclusive: exactly at the cap we stop even if unchanged.
        assert!(!should_continue_polling(PRE_PASTE_POLL_CAP_MS, false));
        assert!(!should_continue_polling(PRE_PASTE_POLL_CAP_MS + 5, false));
    }

    #[test]
    fn polling_continues_while_unchanged_and_under_cap() {
        assert!(should_continue_polling(0, false));
        assert!(should_continue_polling(PRE_PASTE_POLL_CAP_MS - 1, false));
    }

    #[test]
    fn post_paste_sleep_is_bounded() {
        // The clipboard-restore delay must stay within the tightened budget.
        assert!(POST_PASTE_MS <= 120);
    }
}
