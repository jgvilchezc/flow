mod audio;
mod db;
mod format;
mod frontmost;
mod inject;
mod models;
mod postprocess;
mod prompt;
mod settings;
mod stats;
mod stt;

use serde::Serialize;
use settings::Settings;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewWindow};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

const MIN_RECORDING_SECS: f32 = 0.3;
/// whisper.cpp rejects audio shorter than ~1s; short clips are padded.
const MIN_WHISPER_SAMPLES: usize = (audio::WHISPER_SAMPLE_RATE as usize * 12) / 10;

#[derive(Clone, Serialize)]
struct OverlayState {
    state: &'static str,
    message: String,
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

    Ok(PipelineConfig {
        dict_terms,
        replacements,
        snippets,
        style: (active, tone),
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
    let _ = app.emit(
        "flow://state",
        OverlayState {
            state,
            message: message.into(),
        },
    );
}

fn overlay_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("overlay")
}

/// Pins the overlay bottom-center of the primary monitor so it floats above
/// the dock like Flow's pill.
fn show_overlay(app: &AppHandle) {
    if let Some(window) = overlay_window(app) {
        if let Ok(Some(monitor)) = window.primary_monitor() {
            let scale = monitor.scale_factor();
            let size = monitor.size();
            let pos = monitor.position();
            if let Ok(win_size) = window.outer_size() {
                let x = pos.x + (size.width as i32 - win_size.width as i32) / 2;
                let y = pos.y + size.height as i32
                    - win_size.height as i32
                    - (90.0 * scale) as i32;
                let _ = window.set_position(PhysicalPosition::new(x, y));
            }
        }
        let _ = window.show();
    }
}

fn hide_overlay(app: &AppHandle) {
    if let Some(window) = overlay_window(app) {
        let _ = window.hide();
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
            emit_state(app, "recording", "");
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

/// Resolves the formatter style fragment for the active `(context, tone)` pair.
/// Unrecognized values (shouldn't happen — the DB CHECK constraints enforce the
/// vocabulary) resolve to `None`, leaving the base prompt untouched.
fn style_fragment_for(style: &(String, String)) -> Option<&'static str> {
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
    Some(prompt::style_fragment(tone, context))
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

        let mut padded = samples;
        if padded.len() < MIN_WHISPER_SAMPLES {
            padded.resize(MIN_WHISPER_SAMPLES, 0.0);
        }

        // STT bias from the user's vocabulary terms. Zero terms => None => an
        // unbiased transcription identical to before management-ui.
        let bias = prompt::stt_initial_prompt(&cfg.dict_terms);
        match stt::transcribe(&whisper, &settings, bias.as_deref(), padded).await {
            Ok(raw) if raw.is_empty() => {
                emit_state(&app, "idle", "");
                hide_overlay(&app);
            }
            Ok(raw) => {
                // Format with proper-noun preservation and the active style
                // fragment. On any formatter failure `format` returns the raw
                // transcript — expansion still applies to that raw text below.
                let fragment = style_fragment_for(&cfg.style);
                let formatted =
                    format::format(&settings, &raw, &cfg.dict_terms, fragment).await;
                // Deterministic replacements + snippet expansion run on the
                // final text (LLM output or raw fallback alike).
                let final_text =
                    postprocess::apply(&formatted, &cfg.replacements, &cfg.snippets);

                if let Err(err) = inject_on_main_thread(&app, final_text.clone()).await {
                    log::error!("injection failed: {err:#}");
                    emit_state(&app, "error", format!("Paste failed: {err}"));
                } else {
                    emit_state(&app, "idle", "");
                }
                hide_overlay(&app);

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

#[tauri::command]
fn check_accessibility() -> bool {
    inject::ensure_accessibility()
}

#[tauri::command]
async fn test_format(state: tauri::State<'_, AppState>, text: String) -> Result<String, String> {
    let settings = state.settings.lock().unwrap().clone();
    Ok(format::format(&settings, &text, &[], None).await)
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
            check_accessibility,
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

            // Show the main window on first run so the user downloads a model.
            let first_run = !models::REGISTRY.iter().any(|m| models::is_downloaded(m.key));
            if first_run {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
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
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
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
    fn style_fragment_resolves_known_pairs() {
        let style = ("work".to_string(), "formal".to_string());
        let fragment = style_fragment_for(&style).expect("known pair resolves");
        assert_eq!(fragment, prompt::style_fragment(prompt::Tone::Formal, prompt::Context::Work));
    }

    #[test]
    fn style_fragment_unknown_pair_is_none() {
        assert!(style_fragment_for(&("bogus".into(), "casual".into())).is_none());
        assert!(style_fragment_for(&("work".into(), "bogus".into())).is_none());
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
}
