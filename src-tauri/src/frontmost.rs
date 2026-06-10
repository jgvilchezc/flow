//! Captures the name of the frontmost application at dictation time.
//!
//! Flow records which app the user was dictating into so the insights view can
//! roll usage up per app. The lookup goes through `NSWorkspace`, which reflects
//! the app that owns the menu bar (the one that will receive the paste). The
//! call must happen on the main thread while the target app is still frontmost
//! — the global-shortcut handler runs on the main thread, so capture is wired
//! into `start_recording` before the recorder takes over.
//!
//! Any failure (no frontmost app, a nameless background process) resolves to
//! `None`: the app name is metadata for stats, never load-bearing for the
//! dictation itself, so it must never block or panic the pipeline.

use objc2_app_kit::NSWorkspace;

/// Returns the localized name of the frontmost application, or `None` when it
/// cannot be determined.
///
/// Should be called on the main thread (AppKit requirement). On any failure
/// path the result is `None` and dictation proceeds with an unknown app.
pub fn frontmost_app_name() -> Option<String> {
    let workspace = NSWorkspace::sharedWorkspace();
    let app = workspace.frontmostApplication()?;
    let name = app.localizedName()?;
    Some(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Logic guard: capturing the frontmost app must never panic and must
    /// return the `Option<String>` shape. The actual value depends on the GUI
    /// session (none in CI / headless test runs), so only the contract is
    /// asserted here — real capture is verified manually.
    #[test]
    fn returns_none_shape() {
        let result = frontmost_app_name();
        // Either a name or None — both are valid; the point is it doesn't panic
        // and the type is what the pipeline expects.
        match result {
            Some(name) => assert!(!name.is_empty(), "a returned name must be non-empty"),
            None => {}
        }
    }
}
