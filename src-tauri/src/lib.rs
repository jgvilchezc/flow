mod audio;
mod db;
mod format;
mod frontmost;
mod http;
mod inject;
mod models;
mod postprocess;
mod prompt;
mod quickclean;
mod settings;
mod stats;
mod stt;
mod update;

use serde::Serialize;
use settings::Settings;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, WebviewWindow};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

const MIN_RECORDING_SECS: f32 = 0.3;
/// whisper.cpp rejects audio shorter than ~1s; short clips are padded.
const MIN_WHISPER_SAMPLES: usize = (audio::WHISPER_SAMPLE_RATE as usize * 12) / 10;

#[derive(Clone, Serialize)]
struct OverlayState {
    state: &'static str,
    message: String,
    /// The resolved formatting mode label for a `recording` state
    /// (`prompt_engineer` | `style`), so the pill can show "Prompt Engineer"
    /// instead of "Listening". `None` (and omitted from the payload) for every
    /// non-recording state.
    #[serde(skip_serializing_if = "Option::is_none")]
    mode: Option<&'static str>,
}

/// Payload of the `flow://result` event — the post-dictation "changes" card.
/// Emitted only when the formatter actually changed the transcript, so the
/// overlay can show a Wispr-style diff of what was polished.
#[derive(Clone, Serialize)]
struct OverlayResult {
    raw: String,
    formatted: String,
    mode: &'static str,
    app: Option<String>,
}

/// The slice of durable configuration the dictation pipeline needs on every
/// run, snapshotted from the database. Held behind a mutex in [`AppState`] and
/// rebuilt via [`rebuild_pipeline_cfg`] whenever a mutating command changes the
/// underlying tables, so `stop_and_process` never touches SQLite on the hot
/// path.
#[derive(Clone, Default)]
struct PipelineConfig {
    /// Dictionary `term` phrases in recency order (newest first) — fed to the
    /// STT initial prompt and the formatter's proper-noun preservation line.
    dict_terms: Vec<String>,
    /// `(from, to)` literal replacement rules for the post-processing pass.
    replacements: Vec<(String, String)>,
    /// `(trigger, expansion)` snippet rules for the post-processing pass.
    snippets: Vec<(String, String)>,
    /// `(active_context, tone)` driving the formatter's style fragment.
    style: (String, String),
    /// `(app_name, mode)` per-app formatting overrides. `mode` is
    /// `prompt_engineer` or `style`; anything absent falls back to the style
    /// pass. Drives [`resolve_mode`] on the hot path.
    app_mode_map: Vec<(String, String)>,
}

/// Rebuilds the [`PipelineConfig`] snapshot from the database. Called at
/// startup and after every mutating command so the in-memory snapshot stays in
/// sync with persisted dictionary / snippet / style edits.
fn rebuild_pipeline_cfg(conn: &rusqlite::Connection) -> rusqlite::Result<PipelineConfig> {
    let dictionary = db::list_dictionary(conn)?;
    // Terms bias STT/formatter; replacements are literal substitutions. The
    // dictionary lists newest-first, which is exactly the recency order the STT
    // prompt builder expects.
    let mut dict_terms = Vec::new();
    let mut replacements = Vec::new();
    for entry in dictionary {
        match entry.kind.as_str() {
            "term" => dict_terms.push(entry.phrase),
            "replacement" => {
                if let Some(to) = entry.replacement {
                    replacements.push((entry.phrase, to));
                }
            }
            _ => {}
        }
    }

    let snippets = db::list_snippets(conn)?
        .into_iter()
        .map(|s| (s.trigger, s.expansion))
        .collect();

    let style_cfg = db::get_style_config(conn)?;
    let active = style_cfg.active_context;
    let tone = style_cfg
        .contexts
        .iter()
        .find(|c| c.context == active)
        .map(|c| c.tone.clone())
        .unwrap_or_else(|| "casual".to_string());

    let app_mode_map = db::list_app_mode_map(conn)?;

    Ok(PipelineConfig {
        dict_terms,
        replacements,
        snippets,
        style: (active, tone),
        app_mode_map,
    })
}

struct AppState {
    settings: Mutex<Settings>,
    recorder: Mutex<audio::Recorder>,
    whisper: Arc<stt::WhisperCache>,
    /// SQLite connection backing history, dictionary, snippets, and style. On a
    /// failed open it falls back to an in-memory database so dictation keeps
    /// working (history simply won't survive a restart).
    db: Mutex<rusqlite::Connection>,
    /// Snapshot of the durable pipeline config; rebuilt from `db` on every
    /// mutating command via [`rebuild_pipeline_cfg`].
    pipeline_cfg: Mutex<PipelineConfig>,
    processing: Mutex<bool>,
    /// Frontmost app captured at hotkey-press time, consumed once when the
    /// transcript is recorded into history. `None` when capture failed.
    pending_app: Mutex<Option<String>>,
}

/// Opens the durable database at `config_dir()/flow.db`. On any failure
/// (missing directory, permissions, corruption) it logs and falls back to an
/// in-memory database so the app keeps running — dictation never depends on a
/// healthy disk, only persistence does.
fn open_database() -> rusqlite::Connection {
    let path = settings::config_dir().join("flow.db");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match db::open(&path) {
        Ok(conn) => conn,
        Err(err) => {
            log::error!(
                "failed to open database at {}: {err:#} — using in-memory fallback",
                path.display()
            );
            let conn = rusqlite::Connection::open_in_memory()
                .expect("in-memory SQLite must always open");
            db::migrate(&conn).expect("in-memory migration must succeed");
            conn
        }
    }
}

fn emit_state(app: &AppHandle, state: &'static str, message: impl Into<String>) {
    emit_state_full(app, state, message, None);
}

/// Emits `flow://state` with an optional `mode` label. Only the `recording`
/// state carries a mode (`prompt_engineer` | `style`); everything else passes
/// `None`, which is omitted from the serialized payload.
fn emit_state_full(
    app: &AppHandle,
    state: &'static str,
    message: impl Into<String>,
    mode: Option<&'static str>,
) {
    let _ = app.emit(
        "flow://state",
        OverlayState {
            state,
            message: message.into(),
            mode,
        },
    );
}

fn overlay_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("overlay")
}

/// Resizes the overlay window. `resizable: false` in `tauri.conf.json` only
/// blocks *user* resizing, but to keep programmatic resizes robust across
/// platforms we briefly re-enable the flag around the call and restore it.
fn set_overlay_size(window: &WebviewWindow, width: f64, height: f64) {
    let _ = window.set_resizable(true);
    let _ = window.set_size(LogicalSize::new(width, height));
    let _ = window.set_resizable(false);
}

/// Restores the overlay to its compact pill geometry and re-enables
/// click-through. Used both when a fresh dictation starts and when the
/// post-dictation card is dismissed, so the window never lingers card-sized.
fn restore_pill(window: &WebviewWindow) {
    let _ = window.set_ignore_cursor_events(true);
    set_overlay_size(window, 240.0, 56.0);
}

/// Pins the overlay bottom-center of the primary monitor so it floats above
/// the dock like Flow's pill.
fn position_overlay(window: &WebviewWindow) {
    if let Ok(Some(monitor)) = window.primary_monitor() {
        let scale = monitor.scale_factor();
        let size = monitor.size();
        let pos = monitor.position();
        if let Ok(win_size) = window.outer_size() {
            let x = pos.x + (size.width as i32 - win_size.width as i32) / 2;
            let y = pos.y + size.height as i32 - win_size.height as i32 - (90.0 * scale) as i32;
            let _ = window.set_position(PhysicalPosition::new(x, y));
        }
    }
}

/// Shows the overlay as the compact pill. Always restores pill geometry and
/// click-through first, so a pending dictation cleanly takes over from an open
/// result card.
fn show_overlay(app: &AppHandle) {
    if let Some(window) = overlay_window(app) {
        restore_pill(&window);
        position_overlay(&window);
        let _ = window.show();
    }
}

fn hide_overlay(app: &AppHandle) {
    if let Some(window) = overlay_window(app) {
        let _ = window.hide();
    }
}

/// The label the overlay uses to distinguish a prompt-engineer dictation from a
/// style one (drives the pill copy while listening and the card badge).
fn mode_label(mode: &prompt::Mode) -> &'static str {
    match mode {
        prompt::Mode::PromptEngineer => "prompt_engineer",
        prompt::Mode::Style(_) => "style",
    }
}

/// The post-dictation card only appears when the formatter actually changed the
/// text. Comparison is trimmed so trailing-whitespace-only differences (which
/// the user can't see) never pop a redundant card.
fn should_show_result_card(raw: &str, formatted: &str) -> bool {
    raw.trim() != formatted.trim()
}

/// Grows the overlay into the result card, disables click-through so its
/// buttons are usable, and emits `flow://result` with the diff payload. Keeps
/// the window visible — the caller must NOT hide it.
fn show_result_card(
    app: &AppHandle,
    raw: &str,
    formatted: &str,
    mode: &'static str,
    app_name: Option<String>,
) {
    let _ = app.emit(
        "flow://result",
        OverlayResult {
            raw: raw.to_string(),
            formatted: formatted.to_string(),
            mode,
            app: app_name,
        },
    );
    if let Some(window) = overlay_window(app) {
        let _ = window.set_ignore_cursor_events(false);
        set_overlay_size(&window, 460.0, 260.0);
        position_overlay(&window);
        let _ = window.show();
    }
}

fn start_recording(app: &AppHandle) {
    let state = app.state::<AppState>();
    if *state.processing.lock().unwrap() {
        return; // don't stack a new recording on top of an in-flight one
    }
    let mut recorder = state.recorder.lock().unwrap();
    if recorder.is_recording() {
        return; // key-repeat Pressed events while held
    }
    if !inject::ensure_accessibility() {
        emit_state(app, "error", "Grant Accessibility permission to Flow");
        show_overlay(app);
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(3));
            hide_overlay(&app);
        });
        return;
    }
    // Capture the frontmost app BEFORE recording starts, while the user's
    // target app still owns the menu bar. The hotkey handler runs on the main
    // thread, satisfying AppKit's requirement. Failure is non-fatal.
    *state.pending_app.lock().unwrap() = frontmost::frontmost_app_name();
    match recorder.start() {
        Ok(()) => {
            // Resolve the formatting mode from the frontmost app so the pill can
            // show "Prompt Engineer" while listening. Same inputs as the hot
            // path's `resolve_mode`; reads only the cached config, never SQLite.
            let label = {
                let pending = state.pending_app.lock().unwrap();
                let cfg = state.pipeline_cfg.lock().unwrap();
                mode_label(&resolve_mode(
                    pending.as_deref(),
                    &cfg.app_mode_map,
                    &cfg.style,
                ))
            };
            emit_state_full(app, "recording", "", Some(label));
            show_overlay(app);
        }
        Err(err) => {
            log::error!("failed to start recording: {err:#}");
            emit_state(app, "error", format!("Mic error: {err}"));
            show_overlay(app);
            let app = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(3));
                hide_overlay(&app);
            });
        }
    }
}

/// enigo resolves layout-dependent keycodes through the TIS/TSM APIs, which
/// must run on the main thread — macOS 26 enforces this with
/// dispatch_assert_queue and traps the process otherwise.
async fn inject_on_main_thread(app: &AppHandle, text: String) -> anyhow::Result<()> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(inject::inject_text(&text));
    })?;
    rx.await
        .unwrap_or_else(|_| Err(anyhow::anyhow!("paste task dropped")))
}

/// Counts words in `text` using the same alphanumeric tokenizer as the rest of
/// the pipeline (`format`, `postprocess`, `stats`), so word counts are
/// consistent everywhere.
fn word_count(text: &str) -> i64 {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .count() as i64
}

/// Parses the active `(context, tone)` pair into typed style values for the
/// formatter (which derives both the prompt fragment and the example turns).
/// Unrecognized values (shouldn't happen — the DB CHECK constraints enforce the
/// vocabulary) resolve to `None`, leaving the base prompt untouched.
fn parse_style(style: &(String, String)) -> Option<(prompt::Tone, prompt::Context)> {
    let (context, tone) = style;
    let context = match context.as_str() {
        "personal" => prompt::Context::Personal,
        "work" => prompt::Context::Work,
        "email" => prompt::Context::Email,
        "other" => prompt::Context::Other,
        _ => return None,
    };
    let tone = match tone.as_str() {
        "formal" => prompt::Tone::Formal,
        "casual" => prompt::Tone::Casual,
        "very_casual" => prompt::Tone::VeryCasual,
        _ => return None,
    };
    Some((tone, context))
}

/// Resolves the formatting [`prompt::Mode`] for a dictation from the frontmost
/// app. When the frontmost app carries a `prompt_engineer` override in `map`,
/// the dictation is restructured as an AI prompt; every other case — a `style`
/// override, an unmapped app, or an unknown frontmost app (`None`) — falls back
/// to the active style pass. The lookup matches the app's localized name against
/// the `app_mode_map` keys exactly (the seed keys use their canonical spelling).
fn resolve_mode(
    pending_app: Option<&str>,
    map: &[(String, String)],
    style: &(String, String),
) -> prompt::Mode {
    if let Some(app) = pending_app {
        if let Some((_, mode)) = map.iter().find(|(name, _)| name == app) {
            if mode == "prompt_engineer" {
                return prompt::Mode::PromptEngineer;
            }
        }
    }
    prompt::Mode::Style(parse_style(style))
}

/// Decides whether a dictation can skip the LLM formatter and use the fast
/// rule-based quick-clean instead.
///
/// Only [`prompt::Mode::Style`] is eligible — a `PromptEngineer` dictation must
/// always reach the model, so this returns `None` for it regardless of length or
/// settings. For style dictations the decision is delegated to
/// [`quickclean::try_quick_clean`], which returns `None` when quick-clean is
/// disabled, the text is too long, or it carries list/command markers.
/// `Some(cleaned)` means "bypass the formatter and use this text".
fn quick_clean_bypass(
    mode: &prompt::Mode,
    transcript: &str,
    max_words: u32,
    enabled: bool,
) -> Option<String> {
    if !matches!(mode, prompt::Mode::Style(_)) {
        return None;
    }
    quickclean::try_quick_clean(transcript, max_words, enabled)
}

fn stop_and_process(app: &AppHandle) {
    let state = app.state::<AppState>();
    let samples = {
        let mut recorder = state.recorder.lock().unwrap();
        if !recorder.is_recording() {
            return;
        }
        recorder.stop()
    };

    let seconds = samples.len() as f32 / audio::WHISPER_SAMPLE_RATE as f32;
    if seconds < MIN_RECORDING_SECS {
        hide_overlay(app); // accidental tap
        return;
    }

    *state.processing.lock().unwrap() = true;
    emit_state(app, "processing", "");

    // Audio is 16 kHz mono; compute the recorded duration BEFORE padding so
    // short-clip padding never inflates the WPM denominator.
    let recording_ms = (samples.len() / (audio::WHISPER_SAMPLE_RATE as usize / 1000)) as i64;

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let started = Instant::now();
        let state = app.state::<AppState>();
        let settings = state.settings.lock().unwrap().clone();
        let whisper = Arc::clone(&state.whisper);
        // Snapshot the durable pipeline config once for this dictation.
        let cfg = state.pipeline_cfg.lock().unwrap().clone();
        // Snapshot (without consuming) the frontmost app captured at hotkey
        // press so per-app mode resolution can run before formatting. The
        // authoritative `.take()` still happens at the history write below.
        let pending_app_name = state.pending_app.lock().unwrap().clone();

        let mut padded = samples;
        if padded.len() < MIN_WHISPER_SAMPLES {
            padded.resize(MIN_WHISPER_SAMPLES, 0.0);
        }

        // STT bias from the user's vocabulary terms. Zero terms => None => an
        // unbiased transcription identical to before management-ui.
        let bias = prompt::stt_initial_prompt(&cfg.dict_terms);
        let stt_started = Instant::now();
        let stt_result = stt::transcribe(&whisper, &settings, bias.as_deref(), padded).await;
        let stt_ms = stt_started.elapsed().as_millis() as i64;
        match stt_result {
            Ok(raw) if raw.is_empty() => {
                emit_state(&app, "idle", "");
                hide_overlay(&app);
            }
            Ok(raw) => {
                // Format with proper-noun preservation and the active style
                // (prompt fragment + example turns). On any formatter failure
                // `format` returns the raw transcript — expansion still
                // applies to that raw text below.
                let mode =
                    resolve_mode(pending_app_name.as_deref(), &cfg.app_mode_map, &cfg.style);
                // Style dictations short enough for quick-clean skip the LLM
                // entirely; prompt-engineer dictations always reach the model.
                // Either way `format_ms` measures the whole formatting stage.
                let format_started = Instant::now();
                let formatted = match quick_clean_bypass(
                    &mode,
                    &raw,
                    settings.quick_clean_max_words,
                    settings.quick_clean_enabled,
                ) {
                    Some(cleaned) => cleaned,
                    None => format::format(&settings, &raw, &cfg.dict_terms, &mode).await,
                };
                let format_ms = format_started.elapsed().as_millis() as i64;
                // Deterministic replacements + snippet expansion run on the
                // final text (LLM output or quick-clean output alike).
                let final_text =
                    postprocess::apply(&formatted, &cfg.replacements, &cfg.snippets);

                let inject_started = Instant::now();
                let inject_result = inject_on_main_thread(&app, final_text.clone()).await;
                let inject_ms = inject_started.elapsed().as_millis() as i64;
                match inject_result {
                    Err(err) => {
                        log::error!("injection failed: {err:#}");
                        emit_state(&app, "error", format!("Paste failed: {err}"));
                        hide_overlay(&app);
                    }
                    Ok(()) => {
                        // Wispr Polish: when the formatter changed the text, keep
                        // the overlay up as a diff card the user can inspect and
                        // copy from; otherwise behave exactly as before.
                        if should_show_result_card(&raw, &final_text) {
                            show_result_card(
                                &app,
                                &raw,
                                &final_text,
                                mode_label(&mode),
                                pending_app_name.clone(),
                            );
                            // Emit idle AFTER the result event; the frontend gives
                            // the card priority over the idle pill state, so the
                            // card is not clobbered.
                            emit_state(&app, "idle", "");
                        } else {
                            emit_state(&app, "idle", "");
                            hide_overlay(&app);
                        }
                    }
                }

                let pending_app = state.pending_app.lock().unwrap().take();
                let row = db::HistoryRow {
                    id: None,
                    at: now_ms(),
                    raw,
                    word_count: word_count(&final_text),
                    formatted: final_text,
                    duration_ms: started.elapsed().as_millis() as i64,
                    recording_ms,
                    engine: format!("{:?}", settings.stt_engine),
                    app: pending_app,
                    stt_ms: Some(stt_ms),
                    format_ms: Some(format_ms),
                    inject_ms: Some(inject_ms),
                };
                let row = {
                    let conn = state.db.lock().unwrap();
                    match db::insert_history(&conn, &row) {
                        Ok(id) => db::HistoryRow { id: Some(id), ..row },
                        Err(err) => {
                            log::error!("failed to persist history: {err:#}");
                            row
                        }
                    }
                };
                let _ = app.emit("flow://history", row);
            }
            Err(err) => {
                log::error!("transcription failed: {err:#}");
                // unblock the next dictation before the error pill lingers
                *state.processing.lock().unwrap() = false;
                emit_state(&app, "error", format!("{err}"));
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                hide_overlay(&app);
                return;
            }
        }
        *state.processing.lock().unwrap() = false;
    });
}

fn register_hotkey(app: &AppHandle, accelerator: &str) -> anyhow::Result<()> {
    let shortcut: Shortcut = accelerator
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid hotkey '{accelerator}': {e}"))?;
    app.global_shortcut().unregister_all()?;
    app.global_shortcut().register(shortcut)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
fn get_settings(state: tauri::State<AppState>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
fn set_settings(
    app: AppHandle,
    state: tauri::State<AppState>,
    new_settings: Settings,
) -> Result<(), String> {
    let hotkey_changed = {
        let mut current = state.settings.lock().unwrap();
        let changed = current.hotkey != new_settings.hotkey;
        *current = new_settings.clone();
        changed
    };
    settings::save(&new_settings).map_err(|e| e.to_string())?;
    if hotkey_changed {
        register_hotkey(&app, &new_settings.hotkey).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[derive(Serialize)]
struct ModelStatus {
    key: &'static str,
    label: &'static str,
    size_mb: u64,
    downloaded: bool,
}

#[tauri::command]
fn list_models() -> Vec<ModelStatus> {
    models::REGISTRY
        .iter()
        .map(|m| ModelStatus {
            key: m.key,
            label: m.label,
            size_mb: m.size_mb,
            downloaded: models::is_downloaded(m.key),
        })
        .collect()
}

#[tauri::command]
async fn download_model(app: AppHandle, key: String) -> Result<(), String> {
    models::download(app, key).await.map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn get_history(
    state: tauri::State<AppState>,
    limit: Option<u32>,
    before_at: Option<i64>,
) -> Result<Vec<db::HistoryRow>, String> {
    // Default to a 100-row page; keyset paging via `before_at` walks older rows.
    let limit = limit.unwrap_or(100) as i64;
    let conn = state.db.lock().unwrap();
    db::get_history(&conn, limit, before_at).map_err(|e| e.to_string())
}

/// Dismisses the post-dictation result card: hides the overlay and restores it
/// to the compact, click-through pill geometry so the next dictation starts
/// clean. Invoked by the overlay frontend (✕, auto-dismiss, or mouse-leave
/// timeout).
#[tauri::command]
fn dismiss_overlay_result(app: AppHandle) {
    if let Some(window) = overlay_window(&app) {
        let _ = window.hide();
        restore_pill(&window);
    }
}

#[tauri::command]
fn check_accessibility() -> bool {
    // Passive: the Settings view polls this on mount, so it must never
    // trigger the system prompt.
    inject::is_accessibility_granted()
}

#[tauri::command]
fn request_accessibility() -> bool {
    // Explicit user action: shows the system prompt when not yet granted.
    inject::ensure_accessibility()
}

#[tauri::command]
async fn test_format(state: tauri::State<'_, AppState>, text: String) -> Result<String, String> {
    let settings = state.settings.lock().unwrap().clone();
    Ok(format::format(&settings, &text, &[], &prompt::Mode::Style(None)).await)
}

/// Rebuilds the in-memory [`PipelineConfig`] from the database. Call after every
/// mutation so the next dictation sees the change without re-reading SQLite on
/// the hot path. Errors are logged but not surfaced — a stale snapshot is
/// preferable to a failed command.
fn refresh_pipeline_cfg(state: &AppState) {
    let conn = state.db.lock().unwrap();
    match rebuild_pipeline_cfg(&conn) {
        Ok(cfg) => *state.pipeline_cfg.lock().unwrap() = cfg,
        Err(err) => log::error!("failed to rebuild pipeline config: {err:#}"),
    }
}

// ---- insights ----

#[tauri::command]
fn get_stats(state: tauri::State<AppState>) -> Result<stats::Stats, String> {
    let conn = state.db.lock().unwrap();
    stats::get_stats(&conn, chrono::Local::now().date_naive()).map_err(|e| e.to_string())
}

// ---- dictionary ----

#[tauri::command]
fn list_dictionary(state: tauri::State<AppState>) -> Result<Vec<db::DictEntry>, String> {
    let conn = state.db.lock().unwrap();
    db::list_dictionary(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
fn add_dict_entry(
    state: tauri::State<AppState>,
    kind: String,
    phrase: String,
    replacement: Option<String>,
) -> Result<i64, String> {
    let now = now_ms();
    let id = {
        let conn = state.db.lock().unwrap();
        db::add_dictionary(&conn, &kind, &phrase, replacement.as_deref(), now)
            .map_err(|e| e.to_string())?
    };
    refresh_pipeline_cfg(&state);
    Ok(id)
}

#[tauri::command]
fn delete_dict_entry(state: tauri::State<AppState>, id: i64) -> Result<(), String> {
    {
        let conn = state.db.lock().unwrap();
        db::delete_dictionary(&conn, id).map_err(|e| e.to_string())?;
    }
    refresh_pipeline_cfg(&state);
    Ok(())
}

// ---- snippets ----

#[tauri::command]
fn list_snippets(state: tauri::State<AppState>) -> Result<Vec<db::Snippet>, String> {
    let conn = state.db.lock().unwrap();
    db::list_snippets(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
fn upsert_snippet(
    state: tauri::State<AppState>,
    id: Option<i64>,
    trigger: String,
    expansion: String,
) -> Result<(), String> {
    // `id` is accepted for the editor's convenience but the trigger is the
    // natural key — upsert keys on it, so editing the expansion of an existing
    // trigger updates in place regardless of `id`.
    let _ = id;
    {
        let conn = state.db.lock().unwrap();
        db::upsert_snippet(&conn, &trigger, &expansion, now_ms()).map_err(|e| e.to_string())?;
    }
    refresh_pipeline_cfg(&state);
    Ok(())
}

#[tauri::command]
fn delete_snippet(state: tauri::State<AppState>, id: i64) -> Result<(), String> {
    {
        let conn = state.db.lock().unwrap();
        db::delete_snippet(&conn, id).map_err(|e| e.to_string())?;
    }
    refresh_pipeline_cfg(&state);
    Ok(())
}

// ---- style ----

#[tauri::command]
fn get_style(state: tauri::State<AppState>) -> Result<db::StyleConfig, String> {
    let conn = state.db.lock().unwrap();
    db::get_style_config(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_style(state: tauri::State<AppState>, context: String, tone: String) -> Result<(), String> {
    {
        let conn = state.db.lock().unwrap();
        db::set_style(&conn, &context, &tone, now_ms()).map_err(|e| e.to_string())?;
    }
    refresh_pipeline_cfg(&state);
    Ok(())
}

#[tauri::command]
fn set_active_context(state: tauri::State<AppState>, context: String) -> Result<(), String> {
    {
        let conn = state.db.lock().unwrap();
        db::set_active_context(&conn, &context).map_err(|e| e.to_string())?;
    }
    refresh_pipeline_cfg(&state);
    Ok(())
}

// ---- app mode map ----

/// One per-app formatting-mode override. `mode` is `prompt_engineer` or
/// `style`. Mirrors the `(app_name, mode)` pairs from [`db::list_app_mode_map`]
/// as a named struct for the frontend.
#[derive(Serialize)]
struct AppModeEntry {
    app_name: String,
    mode: String,
}

#[tauri::command]
fn get_app_mode_map(state: tauri::State<AppState>) -> Result<Vec<AppModeEntry>, String> {
    let conn = state.db.lock().unwrap();
    let rows = db::list_app_mode_map(&conn).map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|(app_name, mode)| AppModeEntry { app_name, mode })
        .collect())
}

#[tauri::command]
fn set_app_mode(
    state: tauri::State<AppState>,
    app_name: String,
    mode: String,
) -> Result<(), String> {
    // Validate before touching SQLite so the error message is clear; the DB
    // CHECK constraint is the backstop.
    if mode != "prompt_engineer" && mode != "style" {
        return Err(format!("invalid mode '{mode}': expected prompt_engineer or style"));
    }
    {
        let conn = state.db.lock().unwrap();
        db::set_app_mode(&conn, &app_name, &mode, now_ms()).map_err(|e| e.to_string())?;
    }
    refresh_pipeline_cfg(&state);
    Ok(())
}

#[tauri::command]
fn delete_app_mode(state: tauri::State<AppState>, app_name: String) -> Result<(), String> {
    {
        let conn = state.db.lock().unwrap();
        db::delete_app_mode(&conn, &app_name).map_err(|e| e.to_string())?;
    }
    refresh_pipeline_cfg(&state);
    Ok(())
}

/// Current Unix epoch milliseconds — the `created_at` / `updated_at` stamp the
/// DB rows expect.
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// App setup
// ---------------------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| match event.state() {
                    ShortcutState::Pressed => start_recording(app),
                    ShortcutState::Released => stop_and_process(app),
                })
                .build(),
        )
        .manage({
            // Open the durable database, falling back to an in-memory one so a
            // disk/permission failure degrades to a working-but-ephemeral app
            // rather than blocking dictation entirely.
            let conn = open_database();
            let pipeline_cfg = rebuild_pipeline_cfg(&conn).unwrap_or_default();
            AppState {
                settings: Mutex::new(settings::load()),
                recorder: Mutex::new(audio::Recorder::new()),
                whisper: Arc::new(stt::WhisperCache::new()),
                db: Mutex::new(conn),
                pipeline_cfg: Mutex::new(pipeline_cfg),
                processing: Mutex::new(false),
                pending_app: Mutex::new(None),
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            set_settings,
            list_models,
            download_model,
            get_history,
            dismiss_overlay_result,
            check_accessibility,
            request_accessibility,
            test_format,
            get_stats,
            list_dictionary,
            add_dict_entry,
            delete_dict_entry,
            list_snippets,
            upsert_snippet,
            delete_snippet,
            get_style,
            set_style,
            set_active_context,
            get_app_mode_map,
            set_app_mode,
            delete_app_mode,
            update::check_for_update,
        ])
        .setup(|app| {
            // Tray icon with a minimal menu — Flow lives in the menu bar.
            let settings_item =
                MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit Flow", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&settings_item, &quit_item])?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "settings" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            // The overlay must never steal focus or catch clicks.
            if let Some(overlay) = app.get_webview_window("overlay") {
                let _ = overlay.set_ignore_cursor_events(true);
            }

            let handle = app.handle().clone();
            let hotkey = handle
                .state::<AppState>()
                .settings
                .lock()
                .unwrap()
                .hotkey
                .clone();
            if let Err(err) = register_hotkey(&handle, &hotkey) {
                log::error!("failed to register hotkey: {err:#}");
            }

            // A manual launch always surfaces the UI; the tray only takes
            // over after the user closes the window.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }

            // First run: fetch the configured model automatically —
            // dictation must work without a manual download step. Progress
            // streams over the existing download events, so Settings
            // reflects it live; on failure the manual button in Settings
            // remains the recovery path.
            let first_run = !models::REGISTRY.iter().any(|m| models::is_downloaded(m.key));
            if first_run {
                let handle = handle.clone();
                let key = handle
                    .state::<AppState>()
                    .settings
                    .lock()
                    .unwrap()
                    .whisper_model
                    .clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(err) = models::download(handle, key).await {
                        log::error!("auto-download of default model failed: {err:#}");
                    }
                });
            }

            // Best-effort update check. It sleeps first so it never competes
            // with the model load on startup, then emits
            // `flow://update-available` (consumed by the frontend) only when a
            // strictly newer release exists. All failures are silent.
            let update_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                let current = update_handle.package_info().version.to_string();
                if let Some(info) = update::fetch_latest(&current).await {
                    let _ = update_handle.emit("flow://update-available", info);
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the main window hides it; the app lives in the tray.
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // Clicking the Dock icon while the app is already running fires
            // Reopen — standard macOS behavior is to surface the window.
            if let tauri::RunEvent::Reopen { .. } = event {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_count_alphanumeric_tokens() {
        assert_eq!(word_count(""), 0);
        assert_eq!(word_count("   "), 0);
        assert_eq!(word_count("hello world"), 2);
        // Punctuation and symbols are separators, not words. The apostrophe in
        // "don't" splits it into two tokens — same alphanumeric tokenizer the
        // stats/postprocess layers use, kept consistent on purpose.
        assert_eq!(word_count("Hello, world! Nice day."), 4);
        assert_eq!(word_count("don't"), 2);
        // Numbers count as alphanumeric word tokens.
        assert_eq!(word_count("call 555 1234 now"), 4);
    }

    #[test]
    fn style_resolves_known_pairs() {
        let style = ("work".to_string(), "formal".to_string());
        let (tone, context) = parse_style(&style).expect("known pair resolves");
        assert_eq!(tone, prompt::Tone::Formal);
        assert_eq!(context, prompt::Context::Work);
    }

    #[test]
    fn style_unknown_pair_is_none() {
        assert!(parse_style(&("bogus".into(), "casual".into())).is_none());
        assert!(parse_style(&("work".into(), "bogus".into())).is_none());
    }

    #[test]
    fn rebuild_pipeline_cfg_partitions_terms_and_replacements() {
        let conn = db::open_in_memory().unwrap();
        // Terms (no replacement) bias STT; replacements become literal rules.
        db::add_dictionary(&conn, "term", "Tauri", None, 1).unwrap();
        db::add_dictionary(&conn, "term", "rusqlite", None, 2).unwrap();
        db::add_dictionary(&conn, "replacement", "addr", Some("address"), 3).unwrap();
        db::upsert_snippet(&conn, "sig", "Best, Jose", 4).unwrap();
        db::set_style(&conn, "work", "formal", 5).unwrap();
        db::set_active_context(&conn, "work").unwrap();

        let cfg = rebuild_pipeline_cfg(&conn).unwrap();
        assert!(cfg.dict_terms.contains(&"Tauri".to_string()));
        assert!(cfg.dict_terms.contains(&"rusqlite".to_string()));
        assert_eq!(cfg.dict_terms.len(), 2, "only term-kind rows bias STT");
        assert_eq!(cfg.replacements, vec![("addr".to_string(), "address".to_string())]);
        assert_eq!(cfg.snippets, vec![("sig".to_string(), "Best, Jose".to_string())]);
        assert_eq!(cfg.style, ("work".to_string(), "formal".to_string()));
    }

    #[test]
    fn rebuild_pipeline_cfg_defaults_on_empty_db() {
        let conn = db::open_in_memory().unwrap();
        let cfg = rebuild_pipeline_cfg(&conn).unwrap();
        assert!(cfg.dict_terms.is_empty());
        assert!(cfg.replacements.is_empty());
        assert!(cfg.snippets.is_empty());
        // Seeded defaults: personal context, casual tone.
        assert_eq!(cfg.style, ("personal".to_string(), "casual".to_string()));
    }

    fn pe_map() -> Vec<(String, String)> {
        vec![
            ("Warp".to_string(), "prompt_engineer".to_string()),
            ("Mail".to_string(), "style".to_string()),
        ]
    }

    #[test]
    fn resolve_mode_mapped_prompt_engineer() {
        let style = ("work".to_string(), "formal".to_string());
        let mode = resolve_mode(Some("Warp"), &pe_map(), &style);
        assert_eq!(mode, prompt::Mode::PromptEngineer);
    }

    #[test]
    fn resolve_mode_mapped_style_uses_active_style() {
        // An app explicitly mapped to `style` resolves to the parsed style pass.
        let style = ("work".to_string(), "formal".to_string());
        let mode = resolve_mode(Some("Mail"), &pe_map(), &style);
        assert_eq!(
            mode,
            prompt::Mode::Style(Some((prompt::Tone::Formal, prompt::Context::Work)))
        );
    }

    #[test]
    fn resolve_mode_unmapped_app_falls_back_to_style() {
        let style = ("personal".to_string(), "casual".to_string());
        let mode = resolve_mode(Some("Slack"), &pe_map(), &style);
        assert_eq!(
            mode,
            prompt::Mode::Style(Some((prompt::Tone::Casual, prompt::Context::Personal)))
        );
    }

    #[test]
    fn resolve_mode_none_app_falls_back_to_style() {
        // No frontmost app captured -> style pass, never prompt-engineer.
        let style = ("email".to_string(), "very_casual".to_string());
        let mode = resolve_mode(None, &pe_map(), &style);
        assert_eq!(
            mode,
            prompt::Mode::Style(Some((prompt::Tone::VeryCasual, prompt::Context::Email)))
        );
    }

    #[test]
    fn rebuild_pipeline_cfg_includes_seeded_app_mode_map() {
        let conn = db::open_in_memory().unwrap();
        let cfg = rebuild_pipeline_cfg(&conn).unwrap();
        assert_eq!(
            cfg.app_mode_map.len(),
            11,
            "the 11 prompt-engineer seeds must flow into the snapshot"
        );
        assert!(cfg
            .app_mode_map
            .iter()
            .all(|(_, mode)| mode == "prompt_engineer"));
        assert!(cfg
            .app_mode_map
            .iter()
            .any(|(name, _)| name == "Visual Studio Code"));
    }

    #[test]
    fn quick_clean_bypass_prompt_engineer_never_bypasses() {
        // Even a short, enabled, marker-free transcript must still reach the LLM
        // when the resolved mode is prompt-engineer.
        assert_eq!(
            quick_clean_bypass(&prompt::Mode::PromptEngineer, "send the report", 12, true),
            None
        );
    }

    #[test]
    fn quick_clean_bypass_style_bypasses_when_short_and_enabled() {
        let mode = prompt::Mode::Style(Some((prompt::Tone::Casual, prompt::Context::Personal)));
        assert_eq!(
            quick_clean_bypass(&mode, "send the report tomorrow", 12, true),
            Some("Send the report tomorrow.".to_string())
        );
    }

    #[test]
    fn quick_clean_bypass_style_does_not_bypass_when_disabled() {
        let mode = prompt::Mode::Style(None);
        assert_eq!(
            quick_clean_bypass(&mode, "send the report tomorrow", 12, false),
            None
        );
    }

    #[test]
    fn quick_clean_bypass_style_does_not_bypass_with_markers() {
        // List/command markers force the LLM path even for short style text.
        let mode = prompt::Mode::Style(None);
        assert_eq!(
            quick_clean_bypass(&mode, "first buy milk second buy eggs", 20, true),
            None
        );
    }

    #[test]
    fn should_show_result_card_detects_real_changes() {
        // A formatting change pops the card.
        assert!(should_show_result_card("hello world", "Hello, world."));
        // Identical text does not.
        assert!(!should_show_result_card("hello world", "hello world"));
        // Trailing-/leading-whitespace-only differences are invisible to the
        // user, so they must not pop a redundant card.
        assert!(!should_show_result_card("hello world", "  hello world  "));
    }

    #[test]
    fn mode_label_maps_modes() {
        assert_eq!(mode_label(&prompt::Mode::PromptEngineer), "prompt_engineer");
        assert_eq!(mode_label(&prompt::Mode::Style(None)), "style");
        assert_eq!(
            mode_label(&prompt::Mode::Style(Some((
                prompt::Tone::Casual,
                prompt::Context::Personal
            )))),
            "style"
        );
    }

    #[test]
    fn app_mode_map_set_and_delete_reflect_in_fresh_rebuild() {
        let conn = db::open_in_memory().unwrap();
        // Add a new override and confirm the next snapshot sees it.
        db::set_app_mode(&conn, "Slack", "prompt_engineer", 1).unwrap();
        let cfg = rebuild_pipeline_cfg(&conn).unwrap();
        assert_eq!(
            resolve_mode(
                Some("Slack"),
                &cfg.app_mode_map,
                &("personal".to_string(), "casual".to_string())
            ),
            prompt::Mode::PromptEngineer
        );

        // Delete it and confirm a fresh snapshot no longer resolves to PE.
        db::delete_app_mode(&conn, "Slack").unwrap();
        let cfg = rebuild_pipeline_cfg(&conn).unwrap();
        assert_eq!(
            resolve_mode(
                Some("Slack"),
                &cfg.app_mode_map,
                &("personal".to_string(), "casual".to_string())
            ),
            prompt::Mode::Style(Some((prompt::Tone::Casual, prompt::Context::Personal)))
        );
    }
}
