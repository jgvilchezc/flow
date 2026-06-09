import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  DownloadProgress,
  HistoryEntry,
  ModelStatus,
  Settings as SettingsModel,
} from "./types";

const LANGUAGES = [
  { value: "auto", label: "Auto-detect" },
  { value: "es", label: "Español" },
  { value: "en", label: "English" },
  { value: "pt", label: "Português" },
  { value: "fr", label: "Français" },
  { value: "de", label: "Deutsch" },
];

export default function Settings() {
  const [settings, setSettings] = useState<SettingsModel | null>(null);
  const [models, setModels] = useState<ModelStatus[]>([]);
  const [progress, setProgress] = useState<Record<string, DownloadProgress>>({});
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [accessibility, setAccessibility] = useState<boolean | null>(null);
  const [error, setError] = useState<string>("");
  const saveTimer = useRef<ReturnType<typeof setTimeout>>(undefined);

  const refreshModels = useCallback(() => {
    invoke<ModelStatus[]>("list_models").then(setModels).catch(console.error);
  }, []);

  useEffect(() => {
    invoke<SettingsModel>("get_settings").then(setSettings).catch(console.error);
    invoke<HistoryEntry[]>("get_history").then(setHistory).catch(console.error);
    invoke<boolean>("check_accessibility").then(setAccessibility).catch(console.error);
    refreshModels();

    const unlistenProgress = listen<DownloadProgress>(
      "flow://download-progress",
      (event) => {
        setProgress((prev) => ({ ...prev, [event.payload.model]: event.payload }));
        if (event.payload.done) refreshModels();
      },
    );
    const unlistenHistory = listen<HistoryEntry>("flow://history", (event) => {
      setHistory((prev) => [event.payload, ...prev].slice(0, 50));
    });
    return () => {
      unlistenProgress.then((fn) => fn());
      unlistenHistory.then((fn) => fn());
    };
  }, [refreshModels]);

  const update = (patch: Partial<SettingsModel>) => {
    setSettings((prev) => {
      if (!prev) return prev;
      const next = { ...prev, ...patch };
      clearTimeout(saveTimer.current);
      saveTimer.current = setTimeout(() => {
        invoke("set_settings", { newSettings: next }).catch((err) =>
          setError(String(err)),
        );
      }, 400);
      return next;
    });
  };

  const download = (key: string) => {
    setError("");
    invoke("download_model", { key }).catch((err) => setError(String(err)));
  };

  if (!settings) return null;

  return (
    <div className="settings">
      <header className="settings__header">
        <h1>Flow</h1>
        <p>
          Hold <kbd>{settings.hotkey}</kbd> anywhere, speak, release. Your words
          land in the focused input — cleaned up and formatted.
        </p>
      </header>

      {error && <div className="banner banner--error">{error}</div>}
      {accessibility === false && (
        <div className="banner banner--warning">
          Accessibility permission is missing. Enable Flow in System Settings →
          Privacy &amp; Security → Accessibility, then restart the app.
        </div>
      )}

      <section className="card">
        <h2>Speech to text</h2>
        <div className="field">
          <label>Engine</label>
          <div className="segmented">
            <button
              className={settings.stt_engine === "local" ? "active" : ""}
              onClick={() => update({ stt_engine: "local" })}
            >
              Local (whisper.cpp, offline)
            </button>
            <button
              className={settings.stt_engine === "groq" ? "active" : ""}
              onClick={() => update({ stt_engine: "groq" })}
            >
              Groq cloud (free tier, fastest)
            </button>
          </div>
        </div>

        {settings.stt_engine === "local" && (
          <div className="field">
            <label>Whisper model</label>
            <ul className="models">
              {models.map((model) => {
                const p = progress[model.key];
                const downloading = p && !p.done;
                return (
                  <li key={model.key} className="models__row">
                    <label className="models__pick">
                      <input
                        type="radio"
                        name="whisper-model"
                        checked={settings.whisper_model === model.key}
                        disabled={!model.downloaded}
                        onChange={() => update({ whisper_model: model.key })}
                      />
                      <span>
                        {model.label}
                        <small> · {model.size_mb} MB</small>
                      </span>
                    </label>
                    {model.downloaded ? (
                      <span className="models__status">Downloaded</span>
                    ) : downloading ? (
                      <progress value={p.downloaded} max={p.total} />
                    ) : (
                      <button onClick={() => download(model.key)}>Download</button>
                    )}
                  </li>
                );
              })}
            </ul>
          </div>
        )}

        <div className="field">
          <label>Language</label>
          <select
            value={settings.language}
            onChange={(e) => update({ language: e.target.value })}
          >
            {LANGUAGES.map((lang) => (
              <option key={lang.value} value={lang.value}>
                {lang.label}
              </option>
            ))}
          </select>
        </div>
      </section>

      <section className="card">
        <h2>Smart formatting</h2>
        <p className="hint">
          A small LLM rewrites the raw transcript: punctuation, filler removal,
          self-corrections, and lists with colons — the Wispr Flow magic.
        </p>
        <div className="field">
          <label>Formatter</label>
          <div className="segmented">
            <button
              className={settings.formatter === "ollama" ? "active" : ""}
              onClick={() => update({ formatter: "ollama" })}
            >
              Ollama (local)
            </button>
            <button
              className={settings.formatter === "groq" ? "active" : ""}
              onClick={() => update({ formatter: "groq" })}
            >
              Groq (instant)
            </button>
            <button
              className={settings.formatter === "none" ? "active" : ""}
              onClick={() => update({ formatter: "none" })}
            >
              Off
            </button>
          </div>
        </div>
        {settings.formatter === "ollama" && (
          <div className="field">
            <label>Ollama model</label>
            <input
              value={settings.ollama_model}
              onChange={(e) => update({ ollama_model: e.target.value })}
              placeholder="gemma3:4b"
            />
            <p className="hint">
              Pull it first: <code>ollama pull {settings.ollama_model || "gemma3:4b"}</code>
            </p>
          </div>
        )}
        {(settings.formatter === "groq" || settings.stt_engine === "groq") && (
          <>
            <div className="field">
              <label>Groq API key</label>
              <input
                type="password"
                value={settings.groq_api_key}
                onChange={(e) => update({ groq_api_key: e.target.value })}
                placeholder="gsk_…"
              />
              <p className="hint">
                Free at console.groq.com — no card required.
              </p>
            </div>
            {settings.formatter === "groq" && (
              <div className="field">
                <label>Groq LLM model</label>
                <input
                  value={settings.groq_llm_model}
                  onChange={(e) => update({ groq_llm_model: e.target.value })}
                  placeholder="llama-3.1-8b-instant"
                />
              </div>
            )}
          </>
        )}
      </section>

      <section className="card">
        <h2>Hotkey</h2>
        <div className="field">
          <label>Hold to talk</label>
          <input
            value={settings.hotkey}
            onChange={(e) => update({ hotkey: e.target.value })}
            placeholder="Alt+Space"
          />
          <p className="hint">
            Accelerator syntax, e.g. <code>Alt+Space</code> or{" "}
            <code>Ctrl+Shift+D</code>. Press and hold to record, release to
            type. Avoid Cmd-based combos — they can clash with the synthesized
            ⌘V paste.
          </p>
        </div>
      </section>

      <section className="card">
        <h2>History</h2>
        {history.length === 0 ? (
          <p className="hint">Nothing dictated yet. Hold the hotkey and speak.</p>
        ) : (
          <ul className="history">
            {history.map((entry) => (
              <li key={entry.at}>
                <div className="history__formatted">{entry.formatted}</div>
                {entry.raw !== entry.formatted && (
                  <div className="history__raw">{entry.raw}</div>
                )}
                <div className="history__meta">
                  {entry.engine} · {(entry.duration_ms / 1000).toFixed(1)}s
                </div>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
