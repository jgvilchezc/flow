mod audio;
mod db;
mod format;
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
const MAX_HISTORY: usize = 50;

#[derive(Clone, Serialize)]
struct OverlayState {
    state: &'static str,
    message: String,
}

#[derive(Clone, Serialize)]
struct HistoryEntry {
    /// Unix ms — also the stable identity for UI lists.
    at: u128,
    raw: String,
    formatted: String,
    duration_ms: u128,
    engine: String,
}

struct AppState {
    settings: Mutex<Settings>,
    recorder: Mutex<audio::Recorder>,
    whisper: Arc<stt::WhisperCache>,
    history: Mutex<Vec<HistoryEntry>>,
    processing: Mutex<bool>,
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

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let started = Instant::now();
        let state = app.state::<AppState>();
        let settings = state.settings.lock().unwrap().clone();
        let whisper = Arc::clone(&state.whisper);

        let mut padded = samples;
        if padded.len() < MIN_WHISPER_SAMPLES {
            padded.resize(MIN_WHISPER_SAMPLES, 0.0);
        }

        match stt::transcribe(&whisper, &settings, padded).await {
            Ok(raw) if raw.is_empty() => {
                emit_state(&app, "idle", "");
                hide_overlay(&app);
            }
            Ok(raw) => {
                let formatted = format::format(&settings, &raw).await;
                if let Err(err) = inject_on_main_thread(&app, formatted.clone()).await {
                    log::error!("injection failed: {err:#}");
                    emit_state(&app, "error", format!("Paste failed: {err}"));
                } else {
                    emit_state(&app, "idle", "");
                }
                hide_overlay(&app);

                let entry = HistoryEntry {
                    at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis())
                        .unwrap_or_default(),
                    raw,
                    formatted,
                    duration_ms: started.elapsed().as_millis(),
                    engine: format!("{:?}", settings.stt_engine),
                };
                {
                    let mut history = state.history.lock().unwrap();
                    history.insert(0, entry.clone());
                    history.truncate(MAX_HISTORY);
                }
                let _ = app.emit("flow://history", entry);
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
fn get_history(state: tauri::State<AppState>) -> Vec<HistoryEntry> {
    state.history.lock().unwrap().clone()
}

#[tauri::command]
fn check_accessibility() -> bool {
    inject::ensure_accessibility()
}

#[tauri::command]
async fn test_format(state: tauri::State<'_, AppState>, text: String) -> Result<String, String> {
    let settings = state.settings.lock().unwrap().clone();
    Ok(format::format(&settings, &text).await)
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
        .manage(AppState {
            settings: Mutex::new(settings::load()),
            recorder: Mutex::new(audio::Recorder::new()),
            whisper: Arc::new(stt::WhisperCache::new()),
            history: Mutex::new(Vec::new()),
            processing: Mutex::new(false),
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            set_settings,
            list_models,
            download_model,
            get_history,
            check_accessibility,
            test_format,
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
                        if let Some(window) = app.get_webview_window("settings") {
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

            // Show settings on first run so the user downloads a model.
            let first_run = !models::REGISTRY.iter().any(|m| models::is_downloaded(m.key));
            if first_run {
                if let Some(window) = app.get_webview_window("settings") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the settings window hides it; the app lives in the tray.
            if window.label() == "settings" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
