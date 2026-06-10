import { useMemo, useState } from "react";
import type { HistoryEntry } from "../types";
import { cn } from "../lib/cn";

/** A day bucket: a heading plus its entries, newest first within the day. */
interface DayGroup {
  key: string;
  label: string;
  entries: HistoryEntry[];
}

/** Local midnight for the timestamp, as a YYYY-MM-DD key. */
function dayKey(ms: number): string {
  const d = new Date(ms);
  const y = d.getFullYear();
  const m = `${d.getMonth() + 1}`.padStart(2, "0");
  const day = `${d.getDate()}`.padStart(2, "0");
  return `${y}-${m}-${day}`;
}

/** "Today" / "Yesterday" / "Mon, Jun 9" for a day key relative to now. */
function dayLabel(key: string): string {
  const todayKey = dayKey(Date.now());
  const yesterdayKey = dayKey(Date.now() - 86_400_000);
  if (key === todayKey) return "Today";
  if (key === yesterdayKey) return "Yesterday";
  const [y, m, d] = key.split("-").map(Number);
  return new Date(y, m - 1, d).toLocaleDateString(undefined, {
    weekday: "short",
    month: "short",
    day: "numeric",
  });
}

function timeLabel(ms: number): string {
  return new Date(ms).toLocaleTimeString(undefined, {
    hour: "numeric",
    minute: "2-digit",
  });
}

/**
 * Groups history entries into reverse-chronological day buckets. Entries are
 * assumed to arrive newest-first (the command returns DESC by `at`); within a
 * bucket we keep that order. Days appear in newest-first order too.
 */
function groupByDay(entries: HistoryEntry[]): DayGroup[] {
  const groups: DayGroup[] = [];
  const index = new Map<string, DayGroup>();
  for (const entry of entries) {
    const key = dayKey(entry.at);
    let group = index.get(key);
    if (!group) {
      group = { key, label: dayLabel(key), entries: [] };
      index.set(key, group);
      groups.push(group);
    }
    group.entries.push(entry);
  }
  return groups;
}

/** Collapse threshold — rows longer than this get a "Show more" toggle. */
const TRUNCATE_AT = 280;

function FeedRow({ entry }: { entry: HistoryEntry }) {
  const [expanded, setExpanded] = useState(false);
  const text = entry.formatted || entry.raw;
  const long = text.length > TRUNCATE_AT;
  const shown = !expanded && long ? `${text.slice(0, TRUNCATE_AT)}…` : text;

  return (
    <li className="group flex gap-4 rounded-[var(--radius)] px-3 py-2.5 transition-colors duration-150 hover:bg-bg">
      <time
        className="w-20 shrink-0 pt-0.5 text-[12.5px] tabular-nums text-muted"
        dateTime={new Date(entry.at).toISOString()}
      >
        {timeLabel(entry.at)}
      </time>
      <div className="min-w-0 flex-1">
        <p className="select-text whitespace-pre-wrap text-[13.5px] leading-relaxed text-text">
          {shown}
        </p>
        {long && (
          <button
            type="button"
            onClick={() => setExpanded((v) => !v)}
            className="mt-1 cursor-pointer text-[12px] font-medium text-accent outline-none hover:underline focus-visible:underline"
          >
            {expanded ? "Show less" : "Show more"}
          </button>
        )}
      </div>
    </li>
  );
}

export interface HistoryFeedProps {
  entries: HistoryEntry[];
  className?: string;
}

/**
 * The day-grouped dictation feed: a muted day heading, then each dictation as
 * a row with its local time on the left and the formatted text on the right.
 * Long entries collapse with a "Show more" toggle.
 */
export function HistoryFeed({ entries, className }: HistoryFeedProps) {
  const groups = useMemo(() => groupByDay(entries), [entries]);

  return (
    <div className={cn("flex flex-col gap-6", className)}>
      {groups.map((group) => (
        <section key={group.key}>
          <h3 className="mb-1 px-3 text-[12px] font-semibold uppercase tracking-[0.06em] text-muted">
            {group.label}
          </h3>
          <ul className="flex flex-col">
            {group.entries.map((entry) => (
              <FeedRow key={entry.id ?? entry.at} entry={entry} />
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}
