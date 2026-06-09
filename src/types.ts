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
}

export interface OverlayState {
  state: "idle" | "recording" | "processing" | "error";
  message: string;
}
