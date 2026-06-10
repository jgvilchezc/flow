export type SttEngine = "local" | "groq";
export type Formatter = "ollama" | "groq" | "none";

export interface Settings {
  stt_engine: SttEngine;
  whisper_model: string;
  language: string;
  formatter: Formatter;
  ollama_model: string;
  groq_api_key: string;
  groq_llm_model: string;
  hotkey: string;
}

export interface ModelStatus {
  key: string;
  label: string;
  size_mb: number;
  downloaded: boolean;
}

export interface DownloadProgress {
  model: string;
  downloaded: number;
  total: number;
  done: boolean;
}

export interface HistoryEntry {
  at: number;
  raw: string;
  formatted: string;
  duration_ms: number;
  engine: string;
  // Added with the management UI. Optional so older payloads (and code that
  // only reads the original fields) keep type-checking.
  id?: number | null;
  word_count?: number;
  recording_ms?: number;
  app?: string | null;
}

export interface OverlayState {
  state: "idle" | "recording" | "processing" | "error";
  message: string;
}
