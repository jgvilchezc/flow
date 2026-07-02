import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import type {
  AppModeEntry,
  DownloadProgress,
  ModelStatus,
  Settings as SettingsModel,
  UpdateInfo,
} from "../types";
import {
  checkForUpdate,
  deleteAppMode,
  getAppModeMap,
  setAppMode,
} from "../lib/api";
import { Card, CardContent, CardHeader, CardTitle } from "../components/ui/Card";
import { Button } from "../components/ui/Button";
import { Input } from "../components/ui/Input";
import { Tabs } from "../components/ui/Tabs";
import { Spinner } from "../components/ui/Spinner";

const APP_MODES = [
  { value: "prompt_engineer" as const, label: "Prompt Engineer" },
  { value: "style" as const, label: "Default style" },
];

const LANGUAGES = [
  { value: "auto", label: "Auto-detect" },
  { value: "es", label: "Español" },
  { value: "en", label: "English" },
  { value: "pt", label: "Português" },
  { value: "fr", label: "Français" },
  { value: "de", label: "Deutsch" },
];

const STT_ENGINES = [
  { value: "local" as const, label: "Local (whisper.cpp, offline)" },
  { value: "groq" as const, label: "Groq cloud (free tier, fastest)" },
];

const FORMATTERS = [
  { value: "ollama" as const, label: "Ollama (local)" },
  { value: "groq" as const, label: "Groq (instant)" },
  { value: "none" as const, label: "Off" },
];

/** Small caps label above a field, matching the management-UI rhythm. */
function FieldLabel({ children }: { children: React.ReactNode }) {
  return (
    <span className="block text-[11px] font-medium uppercase tracking-[0.06em] text-muted">
      {children}
    </span>
  );
}

/** Light wrapper that gives each field a consistent vertical gap. */
function Field({ children }: { children: React.ReactNode }) {
  return <div className="flex flex-col gap-2">{children}</div>;
}

/**
 * Settings, re-homed into the management shell and restyled on the warm
 * primitives. Behaviour is preserved one-to-one from the legacy `Settings.tsx`:
 * load settings/models/accessibility on mount, debounce `set_settings` by
 * 400ms, surface model-download progress events, and let the user re-register
 * the hotkey by editing the accelerator string. History lives in Home now, so
 * the old history section is gone.
 */
export function SettingsView() {
  const [settings, setSettings] = useState<SettingsModel | null>(null);
  const [models, setModels] = useState<ModelStatus[]>([]);
  const [progress, setProgress] = useState<Record<string, DownloadProgress>>({});
  const [accessibility, setAccessibility] = useState<boolean | null>(null);
  const [error, setError] = useState<string>("");
  const saveTimer = useRef<ReturnType<typeof setTimeout>>(undefined);

  const [appModes, setAppModes] = useState<AppModeEntry[]>([]);
  const [newAppName, setNewAppName] = useState("");
  const [newAppMode, setNewAppMode] =
    useState<AppModeEntry["mode"]>("prompt_engineer");

  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [checking, setChecking] = useState(false);
  const [upToDate, setUpToDate] = useState(false);

  const refreshModels = useCallback(() => {
    invoke<ModelStatus[]>("list_models").then(setModels).catch(console.error);
  }, []);

  const refreshAppModes = useCallback(() => {
    getAppModeMap().then(setAppModes).catch(console.error);
  }, []);

  useEffect(() => {
    invoke<SettingsModel>("get_settings").then(setSettings).catch(console.error);
    invoke<boolean>("check_accessibility")
      .then(setAccessibility)
      .catch(console.error);
    refreshModels();
    refreshAppModes();

    // While the permission is missing, poll so the banner clears itself the
    // moment the user flips the toggle in System Settings — the grant
    // happens outside the app, so a mount-only check goes stale.
    const poll = setInterval(() => {
      invoke<boolean>("check_accessibility")
        .then((granted) => {
          setAccessibility((prev) => (prev === false ? granted : prev));
        })
        .catch(console.error);
    }, 2000);

    const unlistenProgress = listen<DownloadProgress>(
      "flow://download-progress",
      (event) => {
        setProgress((prev) => ({
          ...prev,
          [event.payload.model]: event.payload,
        }));
        if (event.payload.done) refreshModels();
      },
    );
    return () => {
      clearInterval(poll);
      void unlistenProgress.then((fn) => fn());
    };
  }, [refreshModels, refreshAppModes]);

  const addAppMode = () => {
    const name = newAppName.trim();
    if (!name) return;
    setAppMode(name, newAppMode)
      .then(() => {
        setNewAppName("");
        refreshAppModes();
      })
      .catch((err) => setError(String(err)));
  };

  const removeAppMode = (appName: string) => {
    deleteAppMode(appName)
      .then(refreshAppModes)
      .catch((err) => setError(String(err)));
  };

  const runUpdateCheck = () => {
    setChecking(true);
    setUpToDate(false);
    setUpdateInfo(null);
    checkForUpdate()
      .then((info) => {
        if (info) {
          setUpdateInfo(info);
        } else {
          setUpToDate(true);
          setTimeout(() => setUpToDate(false), 4000);
        }
      })
      .catch((err) => setError(String(err)))
      .finally(() => setChecking(false));
  };

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

  if (!settings) {
    return (
      <div className="flex items-center justify-center px-6 py-14">
        <Spinner label="Loading settings" />
      </div>
    );
  }

  return (
    <div className="px-6 py-6">
      <header className="mb-6">
        <h1 className="text-xl font-semibold tracking-tight text-text">Flow</h1>
        <p className="mt-1.5 max-w-prose text-[13.5px] leading-relaxed text-muted">
          Hold{" "}
          <kbd className="rounded-md border border-border bg-bg px-1.5 py-0.5 text-[12px] font-medium text-text">
            {settings.hotkey}
          </kbd>{" "}
          anywhere, speak, release. Your words land in the focused input —
          cleaned up and formatted.
        </p>
      </header>

      {error && (
        <div
          role="alert"
          className="mb-4 rounded-[var(--radius)] border border-red-200 bg-red-50 px-4 py-3 text-[13px] leading-relaxed text-red-700"
        >
          {error}
        </div>
      )}
      {accessibility === false && (
        <div
          role="alert"
          className="mb-4 flex items-center justify-between gap-4 rounded-[var(--radius)] border border-amber-200 bg-amber-50 px-4 py-3 text-[13px] leading-relaxed text-amber-800"
        >
          <span>
            Accessibility permission is missing — Flow needs it to paste
            dictated text. If you already enabled it and rebuilt the app,
            remove Flow from the System Settings list and add it again.
          </span>
          <button
            type="button"
            className="shrink-0 rounded-md bg-amber-800 px-3 py-1.5 text-[12.5px] font-medium text-white"
            onClick={() => {
              invoke<boolean>("request_accessibility")
                .then(setAccessibility)
                .catch(console.error);
            }}
          >
            Grant permission
          </button>
        </div>
      )}

      <div className="flex flex-col gap-4">
        {/* Speech to text -------------------------------------------------- */}
        <Card>
          <CardHeader>
            <CardTitle>Speech to text</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-5">
            <Field>
              <FieldLabel>Engine</FieldLabel>
              <Tabs
                aria-label="Speech-to-text engine"
                items={STT_ENGINES}
                value={settings.stt_engine}
                onChange={(stt_engine) => update({ stt_engine })}
              />
            </Field>

            {settings.stt_engine === "local" && (
              <Field>
                <FieldLabel>Whisper model</FieldLabel>
                <ul className="flex flex-col divide-y divide-border rounded-[var(--radius)] border border-border">
                  {models.map((model) => {
                    const p = progress[model.key];
                    const downloading = p && !p.done;
                    return (
                      <li
                        key={model.key}
                        className="flex items-center justify-between gap-3 px-3 py-2.5"
                      >
                        <label className="flex cursor-pointer items-center gap-2.5 text-[13.5px] text-text">
                          <input
                            type="radio"
                            name="whisper-model"
                            className="accent-accent"
                            checked={settings.whisper_model === model.key}
                            disabled={!model.downloaded}
                            onChange={() =>
                              update({ whisper_model: model.key })
                            }
                          />
                          <span>
                            {model.label}
                            <span className="text-muted">
                              {" "}
                              · {model.size_mb} MB
                            </span>
                          </span>
                        </label>
                        {model.downloaded ? (
                          <span className="text-[12.5px] font-medium text-emerald-600">
                            Downloaded
                          </span>
                        ) : downloading ? (
                          <progress
                            className="w-36 accent-accent"
                            value={p.downloaded}
                            max={p.total}
                          />
                        ) : (
                          <Button
                            size="sm"
                            variant="ghost"
                            onClick={() => download(model.key)}
                          >
                            Download
                          </Button>
                        )}
                      </li>
                    );
                  })}
                </ul>
              </Field>
            )}

            <Field>
              <FieldLabel>Language</FieldLabel>
              <select
                value={settings.language}
                onChange={(e) => update({ language: e.target.value })}
                className="h-10 w-full rounded-[var(--radius)] border border-border bg-surface px-3 text-sm text-text outline-none transition-colors duration-150 focus-visible:border-accent focus-visible:ring-2 focus-visible:ring-accent/30"
              >
                {LANGUAGES.map((lang) => (
                  <option key={lang.value} value={lang.value}>
                    {lang.label}
                  </option>
                ))}
              </select>
            </Field>
          </CardContent>
        </Card>

        {/* Smart formatting ----------------------------------------------- */}
        <Card>
          <CardHeader>
            <CardTitle>Smart formatting</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-5">
            <p className="text-[13px] leading-relaxed text-muted">
              A small LLM rewrites the raw transcript: punctuation, filler
              removal, self-corrections, and lists with colons — the Wispr Flow
              magic.
            </p>
            <Field>
              <FieldLabel>Formatter</FieldLabel>
              <Tabs
                aria-label="Formatter"
                items={FORMATTERS}
                value={settings.formatter}
                onChange={(formatter) => update({ formatter })}
              />
            </Field>
            {settings.formatter === "ollama" && (
              <Field>
                <FieldLabel>Ollama model</FieldLabel>
                <Input
                  value={settings.ollama_model}
                  onChange={(e) => update({ ollama_model: e.target.value })}
                  placeholder="gemma3:4b"
                />
                <p className="text-[12.5px] leading-relaxed text-muted">
                  Pull it first:{" "}
                  <code className="rounded bg-bg px-1.5 py-0.5 text-[12px] text-text">
                    ollama pull {settings.ollama_model || "gemma3:4b"}
                  </code>
                </p>
              </Field>
            )}
            {(settings.formatter === "groq" ||
              settings.stt_engine === "groq") && (
              <>
                <Field>
                  <FieldLabel>Groq API key</FieldLabel>
                  <Input
                    type="password"
                    value={settings.groq_api_key}
                    onChange={(e) => update({ groq_api_key: e.target.value })}
                    placeholder="gsk_…"
                  />
                  {settings.groq_api_key.trim() === "" ? (
                    <div
                      role="alert"
                      className="rounded-[var(--radius)] border border-amber-200 bg-amber-50 px-3 py-2.5 text-[12.5px] leading-relaxed text-amber-800"
                    >
                      Groq is selected but no API key is set — dictation will
                      fail until you paste one.{" "}
                      <button
                        type="button"
                        className="font-medium underline underline-offset-2"
                        onClick={() => {
                          void openUrl("https://console.groq.com/keys");
                        }}
                      >
                        Get a free key
                      </button>{" "}
                      (no card required), or switch back to Local.
                    </div>
                  ) : (
                    <p className="text-[12.5px] leading-relaxed text-muted">
                      Free at console.groq.com — no card required.
                    </p>
                  )}
                </Field>
                {settings.formatter === "groq" && (
                  <Field>
                    <FieldLabel>Groq LLM model</FieldLabel>
                    <Input
                      value={settings.groq_llm_model}
                      onChange={(e) =>
                        update({ groq_llm_model: e.target.value })
                      }
                      placeholder="llama-3.1-8b-instant"
                    />
                  </Field>
                )}
              </>
            )}
          </CardContent>
        </Card>

        {/* Hotkey ---------------------------------------------------------- */}
        <Card>
          <CardHeader>
            <CardTitle>Hotkey</CardTitle>
          </CardHeader>
          <CardContent>
            <Field>
              <FieldLabel>Hold to talk</FieldLabel>
              <Input
                value={settings.hotkey}
                onChange={(e) => update({ hotkey: e.target.value })}
                placeholder="Alt+Space"
              />
              <p className="text-[12.5px] leading-relaxed text-muted">
                Accelerator syntax, e.g.{" "}
                <code className="rounded bg-bg px-1.5 py-0.5 text-[12px] text-text">
                  Alt+Space
                </code>{" "}
                or{" "}
                <code className="rounded bg-bg px-1.5 py-0.5 text-[12px] text-text">
                  Ctrl+Shift+D
                </code>
                . Press and hold to record, release to type. Avoid Cmd-based
                combos — they can clash with the synthesized ⌘V paste.
              </p>
            </Field>
          </CardContent>
        </Card>

        {/* Quick clean ----------------------------------------------------- */}
        <Card>
          <CardHeader>
            <CardTitle>Quick clean</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-5">
            <p className="text-[13px] leading-relaxed text-muted">
              Skip the LLM for very short dictations and just tidy the raw
              transcript — faster for one-liners.
            </p>
            <Field>
              <label className="flex cursor-pointer items-center gap-2.5 text-[13.5px] text-text">
                <input
                  type="checkbox"
                  className="accent-accent"
                  checked={settings.quick_clean_enabled}
                  onChange={(e) =>
                    update({ quick_clean_enabled: e.target.checked })
                  }
                />
                <span>Enable quick clean for short dictations</span>
              </label>
            </Field>
            {settings.quick_clean_enabled && (
              <Field>
                <FieldLabel>Max words</FieldLabel>
                <Input
                  type="number"
                  min={3}
                  max={50}
                  value={settings.quick_clean_max_words}
                  onChange={(e) => {
                    const n = Number.parseInt(e.target.value, 10);
                    if (Number.isNaN(n)) return;
                    update({
                      quick_clean_max_words: Math.min(50, Math.max(3, n)),
                    });
                  }}
                  className="w-28"
                />
                <p className="text-[12.5px] leading-relaxed text-muted">
                  Dictations below this word count skip the formatter
                  (3–50).
                </p>
              </Field>
            )}
          </CardContent>
        </Card>

        {/* Prompt Engineer apps ------------------------------------------- */}
        <Card>
          <CardHeader>
            <CardTitle>Prompt Engineer apps</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-5">
            <p className="text-[13px] leading-relaxed text-muted">
              Override the formatting mode per app. Names must match the macOS
              app name exactly (e.g.{" "}
              <code className="rounded bg-bg px-1.5 py-0.5 text-[12px] text-text">
                Cursor
              </code>
              ).
            </p>
            {appModes.length > 0 && (
              <ul className="flex flex-col divide-y divide-border rounded-[var(--radius)] border border-border">
                {appModes.map((entry) => (
                  <li
                    key={entry.app_name}
                    className="flex items-center justify-between gap-3 px-3 py-2.5"
                  >
                    <span className="min-w-0 flex-1 truncate text-[13.5px] text-text">
                      {entry.app_name}
                    </span>
                    <span className="text-[12.5px] text-muted">
                      {APP_MODES.find((m) => m.value === entry.mode)?.label ??
                        entry.mode}
                    </span>
                    <Button
                      size="sm"
                      variant="danger"
                      onClick={() => removeAppMode(entry.app_name)}
                    >
                      Remove
                    </Button>
                  </li>
                ))}
              </ul>
            )}
            <div className="flex items-end gap-2">
              <Field>
                <FieldLabel>App name</FieldLabel>
                <Input
                  value={newAppName}
                  onChange={(e) => setNewAppName(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") addAppMode();
                  }}
                  placeholder="Cursor"
                />
              </Field>
              <Field>
                <FieldLabel>Mode</FieldLabel>
                <select
                  value={newAppMode}
                  onChange={(e) =>
                    setNewAppMode(e.target.value as AppModeEntry["mode"])
                  }
                  className="h-10 rounded-[var(--radius)] border border-border bg-surface px-3 text-sm text-text outline-none transition-colors duration-150 focus-visible:border-accent focus-visible:ring-2 focus-visible:ring-accent/30"
                >
                  {APP_MODES.map((m) => (
                    <option key={m.value} value={m.value}>
                      {m.label}
                    </option>
                  ))}
                </select>
              </Field>
              <Button onClick={addAppMode} disabled={!newAppName.trim()}>
                Add
              </Button>
            </div>
          </CardContent>
        </Card>

        {/* Updates --------------------------------------------------------- */}
        <Card>
          <CardHeader>
            <CardTitle>Updates</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            <div className="flex items-center gap-3">
              <Button variant="ghost" onClick={runUpdateCheck} disabled={checking}>
                {checking ? "Checking…" : "Check for updates"}
              </Button>
              {upToDate && (
                <span className="text-[13px] text-muted">
                  You&rsquo;re up to date.
                </span>
              )}
            </div>
            {updateInfo && (
              <div className="flex flex-col gap-3 rounded-[var(--radius)] border border-accent/30 bg-accent-soft px-4 py-3">
                <p className="text-[13.5px] font-medium text-text">
                  Flow {updateInfo.version} is available.
                </p>
                {updateInfo.notes && (
                  <p className="whitespace-pre-wrap text-[13px] leading-relaxed text-muted">
                    {updateInfo.notes}
                  </p>
                )}
                <div>
                  <Button
                    onClick={() => {
                      void openUrl(updateInfo.url);
                    }}
                  >
                    Download
                  </Button>
                </div>
                <p className="text-[12.5px] leading-relaxed text-muted">
                  After downloading, re-run the Gatekeeper workaround (
                  <code className="rounded bg-bg px-1.5 py-0.5 text-[12px] text-text">
                    xattr -cr
                  </code>{" "}
                  on the app) before opening the new build.
                </p>
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
