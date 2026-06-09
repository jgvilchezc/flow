# Flow

Free, local-first voice dictation for macOS — a Wispr Flow alternative that costs $0.

Hold a hotkey anywhere, speak, release. Your words land in whatever input has focus, cleaned up and formatted: punctuation fixed, filler words removed, self-corrections applied, enumerations turned into lists with colons.

## How it works

```
hold hotkey ──▶ mic capture (cpal, 16kHz mono)
release     ──▶ speech-to-text ──▶ LLM formatting pass ──▶ paste into focused input
                 │                   │                       (clipboard + ⌘V)
                 ├─ local: whisper.cpp (Metal)               clipboard restored after
                 └─ cloud: Groq free tier
```

The "magic" formatting Wispr Flow does is an LLM post-processing pass over the raw transcript. Flow replicates it with a compact system prompt against either:

- **Ollama** (default) — fully local and offline. Recommended model: `gemma3:4b`.
- **Groq** (free tier) — near-instant cloud inference with `llama-3.1-8b-instant`.
- **Off** — raw transcript, no formatting.

## Engines

| | Local (default) | Groq cloud |
|---|---|---|
| Model | whisper.cpp `large-v3-turbo` quantized (Metal) | `whisper-large-v3-turbo` |
| Cost | $0 forever, fully offline | $0 (free tier: 2,000 req/day) |
| Privacy | Audio never leaves your Mac | Audio sent to Groq |
| Speed (~10s clip, M-series) | < 1s | ~300ms + network |

## Setup

Requirements: macOS (Apple Silicon), Rust, Node 18+, pnpm, cmake.

```bash
pnpm install
pnpm tauri dev      # development
pnpm tauri build    # release bundle (Flow.app)
```

First run:

1. The settings window opens — download a Whisper model (Large v3 Turbo recommended, 574 MB) or paste a Groq API key (free at console.groq.com) and switch the engine to Groq.
2. For local formatting: `ollama pull gemma3:4b` (install Ollama from ollama.com if needed).
3. Grant permissions when macOS asks:
   - **Microphone** — to record your voice.
   - **Accessibility** (System Settings → Privacy & Security → Accessibility) — to synthesize the ⌘V keystroke that types the text for you.
4. Hold `Option+Space` (configurable), speak, release.

## Free-tier limits worth knowing

- **Groq STT**: 2,000 requests/day, 7,200 audio-seconds/hour, org-level.
- **Groq LLM** (`llama-3.1-8b-instant`): 30 RPM, 14,400 req/day, 500k tokens/day.
- **Cerebras** is an alternative free LLM endpoint (1M tokens/day, OpenAI-compatible) if you ever exhaust Groq — point the base URL in `src-tauri/src/format.rs` at it.
- Local mode has no limits. That's the point.

## Architecture notes

- **Tauri 2** menu-bar app (`LSUIElement`, no dock icon). Two windows: `settings` and a transparent, non-focusable `overlay` pill that never steals focus from the input you're dictating into.
- **Hold-to-talk** via `tauri-plugin-global-shortcut` `ShortcutState::Pressed/Released`. Bare-modifier hotkeys (e.g. just `Fn` or right-`⌥`) need a CGEventTap — planned, see roadmap.
- **Text injection** uses clipboard + synthesized ⌘V (`arboard` + `enigo`), the only approach that works reliably across native apps, Electron, web views and terminals. The previous clipboard is restored afterwards.
- **Whisper context is cached** between dictations (model load + Metal init is the expensive part), so only the first dictation after launch pays the load cost.
- Formatting failures (Ollama down, rate limit) **fall back to the raw transcript** — dictation never blocks on the formatter.

## Prompt regression testing

`scripts/` contains benchmarks that extract the formatting `SYSTEM_PROMPT` straight from `format.rs` (no drift) and run it against Ollama:

```bash
node scripts/bench_format.mjs gemma3:4b   # canonical cases: lists, corrections, fillers
node scripts/e2e.mjs                      # real whisper transcripts, both models
```

Run them after any prompt change — they caught a few-shot contamination bug that a plain read-through would have missed.

## Known limitations

- The Groq API key is stored in plaintext at `~/Library/Application Support/flow/settings.json` (Keychain support is on the roadmap). Local mode needs no key at all.
- Avoid `Cmd`-based hotkeys: the app synthesizes ⌘V to paste, and a held Cmd can clash with it.

## Roadmap

- [ ] Store the Groq API key in the macOS Keychain
- [ ] CGEventTap hotkeys: hold bare `Fn`/right-`⌥` like Wispr Flow (needs Input Monitoring permission + Developer ID signing for release builds)
- [ ] Streaming transcription while speaking
- [ ] Parakeet TDT v3 engine (`parakeet-rs`/ONNX) — faster than Whisper on CPU
- [ ] Per-app tone profiles (casual in Slack, formal in Mail)
- [ ] Custom vocabulary / name boosting
