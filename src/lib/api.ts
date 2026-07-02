/**
 * Typed wrappers over the Tauri command surface plus the TypeScript shapes that
 * mirror the Rust serde structs one-to-one. Keeping every `invoke` call in this
 * module means the views never deal with raw command names or argument casing
 * (Tauri camelCases Rust snake_case args at the JS boundary).
 */
import { invoke } from "@tauri-apps/api/core";
import type { AppModeEntry, HistoryEntry } from "../types";

// ---------------------------------------------------------------------------
// Types — mirror the Rust serde structs exactly.
// ---------------------------------------------------------------------------

/** Mirrors `stats::Stats`. Tuples preserve the Rust `Vec<(..)>` ordering. */
export interface Stats {
  total_words: number;
  avg_wpm: number;
  current_streak: number;
  longest_streak: number;
  fixes_made: number;
  /** `(app, words, sessions)`, busiest first, capped at 8. */
  per_app: Array<[string, number, number]>;
  /** `(day "YYYY-MM-DD", words)` for the last 365 days. */
  heatmap: Array<[string, number]>;
}

/** Mirrors `db::DictEntry`. `kind` is `"term"` or `"replacement"`. */
export interface DictEntry {
  id: number | null;
  kind: "term" | "replacement";
  phrase: string;
  replacement: string | null;
  created_at: number;
}

/** Mirrors `db::Snippet`. */
export interface Snippet {
  id: number | null;
  trigger: string;
  expansion: string;
  created_at: number;
}

/** Mirrors `db::StyleContext`. */
export interface StyleContext {
  context: StyleContextKey;
  tone: Tone;
  updated_at: number;
}

/** Mirrors `db::StyleConfig`. */
export interface StyleConfig {
  contexts: StyleContext[];
  active_context: StyleContextKey;
}

export type StyleContextKey = "personal" | "work" | "email" | "other";
export type Tone = "formal" | "casual" | "very_casual";

// ---------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------

export function getHistory(
  limit?: number,
  beforeAt?: number,
): Promise<HistoryEntry[]> {
  return invoke<HistoryEntry[]>("get_history", {
    limit,
    beforeAt,
  });
}

// ---------------------------------------------------------------------------
// Insights
// ---------------------------------------------------------------------------

export function getStats(): Promise<Stats> {
  return invoke<Stats>("get_stats");
}

// ---------------------------------------------------------------------------
// Dictionary
// ---------------------------------------------------------------------------

export function listDictionary(): Promise<DictEntry[]> {
  return invoke<DictEntry[]>("list_dictionary");
}

export function addDictEntry(
  kind: "term" | "replacement",
  phrase: string,
  replacement?: string,
): Promise<number> {
  return invoke<number>("add_dict_entry", { kind, phrase, replacement });
}

export function deleteDictEntry(id: number): Promise<void> {
  return invoke("delete_dict_entry", { id });
}

// ---------------------------------------------------------------------------
// Snippets
// ---------------------------------------------------------------------------

export function listSnippets(): Promise<Snippet[]> {
  return invoke<Snippet[]>("list_snippets");
}

export function upsertSnippet(
  trigger: string,
  expansion: string,
  id?: number,
): Promise<void> {
  return invoke("upsert_snippet", { id, trigger, expansion });
}

export function deleteSnippet(id: number): Promise<void> {
  return invoke("delete_snippet", { id });
}

// ---------------------------------------------------------------------------
// Style
// ---------------------------------------------------------------------------

export function getStyle(): Promise<StyleConfig> {
  return invoke<StyleConfig>("get_style");
}

export function setStyle(
  context: StyleContextKey,
  tone: Tone,
): Promise<void> {
  return invoke("set_style", { context, tone });
}

export function setActiveContext(context: StyleContextKey): Promise<void> {
  return invoke("set_active_context", { context });
}

// ---------------------------------------------------------------------------
// App mode map (per-app formatting overrides)
// ---------------------------------------------------------------------------

export function getAppModeMap(): Promise<AppModeEntry[]> {
  return invoke<AppModeEntry[]>("get_app_mode_map");
}

export function setAppMode(
  appName: string,
  mode: "prompt_engineer" | "style",
): Promise<void> {
  return invoke("set_app_mode", { appName, mode });
}

export function deleteAppMode(appName: string): Promise<void> {
  return invoke("delete_app_mode", { appName });
}
