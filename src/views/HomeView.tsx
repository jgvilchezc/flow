import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getHistory, getStats, type Stats } from "../lib/api";
import { hotkeyParts } from "../lib/hotkey";
import type { HistoryEntry, Settings } from "../types";
import { useView } from "../app/ViewContext";
import { HistoryFeed } from "../components/HistoryFeed";
import { StatRail } from "../components/StatRail";
import { EmptyState } from "../components/ui/EmptyState";
import { Spinner } from "../components/ui/Spinner";

const MicIcon = (
  <svg
    width="22"
    height="22"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.5"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <rect x="9" y="3" width="6" height="11" rx="3" />
    <path d="M5 11a7 7 0 0 0 14 0M12 18v3" />
  </svg>
);

const ZERO_STATS: Stats = {
  total_words: 0,
  avg_wpm: 0,
  current_streak: 0,
  longest_streak: 0,
  fixes_made: 0,
  per_app: [],
  heatmap: [],
};

/**
 * Home: a welcome header, a compact stat rail, and the day-grouped dictation
 * feed. Both the feed and the stats refetch whenever `dataVersion` bumps — the
 * App shell owns the single `flow://history` listener, so a new dictation
 * refreshes Home without it subscribing to events itself.
 */
function HotkeyChip({ accelerator }: { accelerator: string }) {
  return (
    <span className="inline-flex items-center gap-1 align-baseline">
      {hotkeyParts(accelerator).map((part, i) => (
        <kbd
          key={i}
          className="rounded-md border border-border bg-surface px-1.5 py-0.5 text-[12px] font-medium text-text shadow-[0_1px_0_var(--color-border)]"
        >
          {part}
        </kbd>
      ))}
    </span>
  );
}

export function HomeView() {
  const { dataVersion } = useView();
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [stats, setStats] = useState<Stats>(ZERO_STATS);
  const [loading, setLoading] = useState(true);
  const [hotkey, setHotkey] = useState<string | null>(null);

  useEffect(() => {
    invoke<Settings>("get_settings")
      .then((s) => setHotkey(s.hotkey))
      .catch(console.error);
  }, [dataVersion]);

  useEffect(() => {
    let cancelled = false;
    Promise.all([getHistory(100), getStats()])
      .then(([history, s]) => {
        if (cancelled) return;
        setEntries(history);
        setStats(s);
      })
      .catch(console.error)
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [dataVersion]);

  return (
    <div className="px-6 py-6">
      <header className="mb-6">
        <h1 className="text-xl font-semibold tracking-tight text-text">
          Welcome back
        </h1>
        <p className="mt-1.5 text-[13.5px] leading-relaxed text-muted">
          Hold {hotkey ? <HotkeyChip accelerator={hotkey} /> : "your hotkey"}{" "}
          anywhere, speak, and release. Everything you dictate lands here — you
          can change the key in Settings.
        </p>
      </header>

      <div className="mb-7">
        <StatRail stats={stats} />
      </div>

      {loading ? (
        <div className="flex items-center justify-center px-6 py-14">
          <Spinner label="Loading history" />
        </div>
      ) : entries.length === 0 ? (
        <EmptyState
          icon={MicIcon}
          title="Nothing dictated yet"
          hint={`Hold ${hotkey ? hotkeyParts(hotkey).join(" ") : "your hotkey"} and start talking — your dictations will show up here, grouped by day.`}
        />
      ) : (
        <HistoryFeed entries={entries} />
      )}
    </div>
  );
}
