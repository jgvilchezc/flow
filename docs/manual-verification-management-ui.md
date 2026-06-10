# Manual Verification — management-ui

Manual, GUI-driven checks for the `management-ui` change. Automated gates
(`cargo test`, `cargo build`, `pnpm build`, and `bench_format.mjs`) cover the
Rust pipeline and the formatter; the steps below cover everything that needs a
running app, a real microphone, and the macOS accessibility/paste path.

## Prerequisites

- macOS with Accessibility permission granted to Flow
  (System Settings → Privacy & Security → Accessibility).
- A working microphone.
- For local formatting: `ollama serve` running with the configured model pulled
  (default `gemma3:4b`): `ollama pull gemma3:4b`.
- Build and launch: `pnpm tauri dev` (or a release build).
- A text field to dictate into (Notes, a browser input, etc.).

Each step lists the action and the expected result. Mark PASS/FAIL as you go.

---

## 1. End-to-end dictation regression (incl. overlay)

1. Focus a text input.
2. Press and hold the hotkey (default `Alt+Space`).
   - Expected: the overlay pill appears with a pulsing red dot and animated
     bars while you speak.
3. Say a sentence, then release the hotkey.
   - Expected: the pill switches to a processing spinner, then disappears.
   - Expected: the formatted text is pasted into the focused input via the
     synthesized ⌘V.
4. Confirm the overlay never steals focus and disappears cleanly after paste.

## 2. History persists across app restart

1. Dictate two or three sentences.
2. Open the app window → Home. Confirm the dictations appear in the feed.
3. Fully quit Flow (tray → Quit) and relaunch.
4. Open Home again.
   - Expected: the previous dictations are still listed (SQLite-backed history
     survives restart).

## 3. Home — feed and stats

1. Open Home.
   - Expected: "Welcome back" header.
   - Expected: stat rail shows Words dictated, Avg words / min, Day streak with
     real (non-placeholder) numbers.
2. Confirm the feed is grouped by day with headings "Today" / "Yesterday" /
   weekday-date, newest first, each row showing the local time on the left.
3. With the app window open, dictate a new sentence.
   - Expected: the feed and stats update live (no manual refresh) — the new row
     appears under "Today".
4. Dictate a very long passage.
   - Expected: the row truncates with a "Show more" toggle that expands it.

## 4. Insights widgets

1. Open Insights (after at least a few dictations).
   - Expected: the WPM gauge needle points at your average WPM; the number
     under it matches the rail on Home.
2. Confirm the headline cards: Total words, Words corrected, Current streak,
   Longest streak — all populated.
3. Confirm the per-app bars list each frontmost app with a words/sessions line
   and a proportional bar. If an app was not captured, an "Unknown" bucket is
   shown verbatim (not hidden).
4. Confirm the heatmap renders weekday rows × week columns, with month labels,
   today's cell ring-highlighted, and a text summary line ("N active days · M
   words…"). Hover a cell → tooltip shows the date and word count.

## 5. Dictionary term — STT bias (ES + EN)

1. Open Dictionary → Add new → kind = Term → phrase `Wispr Flow` → Add.
2. ES: dictate `uso wispr flow todos los días`.
   - Expected: the transcript spells `Wispr Flow` exactly (capitalization and
     spelling biased by the term).
3. EN: dictate `i use wispr flow every day`.
   - Expected: again `Wispr Flow` spelled exactly.

## 6. Dictionary replacement

1. Dictionary → Add new → kind = Replacement → phrase `btw` → replace with
   `by the way` → Add.
2. Dictate a sentence containing the spoken token `btw`.
   - Expected: the inserted text reads `by the way` (replacement runs before
     snippets, whole-word only).

## 7. Snippet trigger (alone, mid-sentence, not inside a larger word)

1. Snippets → Add new → trigger `myemail` → expansion `jose@example.com` → Save.
2. Dictate just the trigger: `myemail`.
   - Expected: expands to `jose@example.com`.
3. Dictate it mid-sentence: `send it to myemail please`.
   - Expected: expands to `send it to jose@example.com please`.
4. Dictate a word that contains the trigger as a substring, e.g. `myemailbox`.
   - Expected: NOT expanded — matching is whole-word
     (`char::is_alphanumeric` boundaries).

## 8. Style preset — observable difference (ES + EN)

1. Style → Personal context → select **Formal**. Set Active context = Personal.
2. Dictate an informal sentence (ES): `che mañana paso por tu casa tipo a las ocho`.
   - Expected (formal): full capitalization and complete punctuation.
3. Switch Personal to **Very casual**, dictate the same line.
   - Expected: no leading capitals, minimal punctuation, chat-style.
4. Repeat 2–3 with an informal EN sentence and confirm the register shifts.

> Note: tone differentiation depends on the formatter model. The `--style`
> bench against `gemma3:4b` showed weak separation (formal ≈ casual ≈ very
> casual). Verify with the model you actually run; a stronger model
> (e.g. qwen2.5:7b or a Groq model) is expected to differentiate more clearly.

## 9. Frontmost app recorded per dictation

1. Dictate from two different apps (e.g. Notes, then a browser).
2. Open Insights → per-app bars.
   - Expected: both apps appear with their own words/sessions counts attributed
     to the app that was frontmost at hotkey press.

## 10. Ollama-down raw fallback still expands snippets

1. Stop Ollama (`Ctrl+C` on `ollama serve`), or set Formatter = Off in Settings.
2. Ensure a snippet exists (e.g. `myemail` from step 7).
3. Dictate `email me at myemail`.
   - Expected: the LLM formatting is skipped (raw transcript), but the
     deterministic post-processing still runs — `myemail` expands to
     `jose@example.com`.

## 11. Settings preserved + first-run + tray + close-hide

1. Settings: change language, formatter, Ollama model, and hotkey. Wait ~1s for
   the debounced save.
2. Quit and relaunch.
   - Expected: every setting is preserved.
3. Confirm the new hotkey is the one that now triggers dictation (re-registered).
4. First-run: with a fresh config (or first launch), confirm the accessibility
   banner appears if permission is missing, and the model list offers downloads.
5. Tray: click the tray icon → the menu shows the dictation entries and a
   Settings action that opens/raises the main window.
6. Close-hide: click the window's close button.
   - Expected: the window hides (app keeps running in the tray), it does not
     quit. Re-open from the tray.

---

## Coverage note

Steps 5–8 and 10 exercise behaviour also covered by Rust unit tests for the
deterministic post-processor and prompt builders; the manual steps confirm the
end-to-end path (mic → STT → format → paste) that tests cannot reach.
