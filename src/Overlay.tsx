import { useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import type { OverlayResult, OverlayState } from "./types";
import { wordDiff } from "./lib/diff";

/** Auto-dismiss the result card after this idle time; shorter after a hover. */
const AUTO_DISMISS_MS = 12_000;
const RESUME_DISMISS_MS = 5_000;

export default function Overlay() {
  const [overlay, setOverlay] = useState<OverlayState>({
    state: "idle",
    message: "",
  });
  const [result, setResult] = useState<OverlayResult | null>(null);
  const [copied, setCopied] = useState<"formatted" | "raw" | null>(null);
  const timerRef = useRef<number | null>(null);

  // Timer/dismiss helpers only touch the stable `timerRef` and setState
  // dispatchers, so the listeners registered once below never capture stale
  // closures.
  const clearTimer = () => {
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  };

  const dismiss = () => {
    clearTimer();
    setResult(null);
    setCopied(null);
    void invoke("dismiss_overlay_result");
  };

  const armTimer = (ms: number) => {
    clearTimer();
    timerRef.current = window.setTimeout(dismiss, ms);
  };

  useEffect(() => {
    const unlistenState = listen<OverlayState>("flow://state", (event) => {
      setOverlay(event.payload);
      // A new dictation takes over: drop any open card locally so the pill
      // shows immediately. The backend already restored the window to pill
      // geometry before emitting this event.
      if (event.payload.state === "recording") {
        clearTimer();
        setResult(null);
        setCopied(null);
      }
    });

    const unlistenResult = listen<OverlayResult>("flow://result", (event) => {
      // Guard against a no-op payload (nothing the diff would flag): mirror the
      // idle path and dismiss instead of showing an empty card.
      const changes = wordDiff(
        event.payload.raw,
        event.payload.formatted,
      ).filter((s) => s.type !== "equal").length;
      if (changes === 0) {
        setResult(null);
        void invoke("dismiss_overlay_result");
        return;
      }
      setResult(event.payload);
      setCopied(null);
      armTimer(AUTO_DISMISS_MS);
    });

    return () => {
      unlistenState.then((fn) => fn());
      unlistenResult.then((fn) => fn());
      clearTimer();
    };
  }, []);

  const segments = useMemo(
    () => (result ? wordDiff(result.raw, result.formatted) : []),
    [result],
  );
  const changes = segments.filter((s) => s.type !== "equal").length;

  const copy = (which: "formatted" | "raw") => {
    if (!result) return;
    const text = which === "formatted" ? result.formatted : result.raw;
    void navigator.clipboard.writeText(text).then(() => {
      setCopied(which);
      setTimeout(() => setCopied(null), 1500);
    });
  };

  // The result card takes priority over the idle pill state — while a card is
  // open the pill state machine (which emits idle right after the result) must
  // not clobber it.
  if (result) {
    return (
      <div
        className="card"
        onMouseEnter={clearTimer}
        onMouseLeave={() => armTimer(RESUME_DISMISS_MS)}
      >
        <div className="card__header">
          <span className="card__title">
            {changes} {changes === 1 ? "Change" : "Changes"}
          </span>
          <button
            type="button"
            className="card__close"
            onClick={dismiss}
            aria-label="Dismiss"
          >
            ✕
          </button>
        </div>
        <div className="card__body">
          <p className="card__diff">
            {segments.map((seg, idx) => {
              if (seg.type === "add") {
                return (
                  <span key={idx} className="card__add">
                    {seg.text}
                  </span>
                );
              }
              if (seg.type === "remove") {
                return (
                  <span key={idx} className="card__remove">
                    {seg.text}
                  </span>
                );
              }
              return <span key={idx}>{seg.text}</span>;
            })}
          </p>
        </div>
        <div className="card__footer">
          <button
            type="button"
            className="card__btn"
            onClick={() => copy("formatted")}
          >
            {copied === "formatted" ? "Copied" : "Copy"}
          </button>
          <button
            type="button"
            className="card__btn"
            onClick={() => copy("raw")}
          >
            {copied === "raw" ? "Copied" : "Copy raw"}
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className={`pill pill--${overlay.state}`}>
      {overlay.state === "recording" && (
        <>
          <span className="pill__dot" />
          <span className="pill__bars">
            <i />
            <i />
            <i />
            <i />
            <i />
          </span>
          <span
            className={`pill__label${
              overlay.mode === "prompt_engineer" ? " pill__label--pe" : ""
            }`}
          >
            {overlay.mode === "prompt_engineer" ? "Prompt Engineer" : "Listening"}
          </span>
        </>
      )}
      {overlay.state === "processing" && (
        <>
          <span className="pill__spinner" />
          <span className="pill__label">Transcribing…</span>
        </>
      )}
      {overlay.state === "error" && (
        <span className="pill__label pill__label--error">
          {overlay.message || "Something went wrong"}
        </span>
      )}
    </div>
  );
}
