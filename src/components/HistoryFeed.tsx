import { useMemo, useState } from "react";
import type { HistoryEntry } from "../types";
import { wordDiff, type DiffSegment } from "../lib/diff";
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

/** "STT 320ms · Format 410ms · Inject 95ms" — omits any stage that is null. */
function timingLabel(entry: HistoryEntry): string {
  const parts: string[] = [];
  if (entry.stt_ms != null) parts.push(`STT ${entry.stt_ms}ms`);
  if (entry.format_ms != null) parts.push(`Format ${entry.format_ms}ms`);
  if (entry.inject_ms != null) parts.push(`Inject ${entry.inject_ms}ms`);
  return parts.join(" · ");
}

/**
 * Renders diff segments inline: removals struck through and muted, additions on
 * a subtle accent wash, equal text plain. Preserves the source spacing.
 */
function DiffText({ segments }: { segments: DiffSegment[] }) {
  return (
    <p className="select-text whitespace-pre-wrap text-[13.5px] leading-relaxed text-text">
      {segments.map((seg, idx) => {
        if (seg.type === "add") {
          return (
            <span
              key={idx}
              className="rounded-[3px] bg-accent-soft text-accent"
            >
              {seg.text}
            </span>
          );
        }
        if (seg.type === "remove") {
          return (
            <span key={idx} className="text-muted line-through">
              {seg.text}
            </span>
          );
        }
        return <span key={idx}>{seg.text}</span>;
      })}
    </p>
  );
}

function FeedRow({ entry }: { entry: HistoryEntry }) {
  const [expanded, setExpanded] = useState(false);
  const [showDiff, setShowDiff] = useState(false);
  const [copied, setCopied] = useState(false);

  const raw = entry.raw ?? "";
  const formatted = entry.formatted || raw;
  // A diff is only meaningful when we have a raw transcript that actually
  // differs from the formatted output. Legacy rows with no/equal raw fall back
  // to the plain formatted text.
  const hasDiff = raw.length > 0 && raw !== formatted;
  const segments = useMemo(
    () => (hasDiff ? wordDiff(raw, formatted) : []),
    [hasDiff, raw, formatted],
  );

  const long = formatted.length > TRUNCATE_AT;
  const shown =
    !expanded && long ? `${formatted.slice(0, TRUNCATE_AT)}…` : formatted;
  const timings = timingLabel(entry);

  const copyRaw = () => {
    void navigator.clipboard.writeText(raw).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  };

  return (
    <li className="group flex gap-4 rounded-[var(--radius)] px-3 py-2.5 transition-colors duration-150 hover:bg-bg">
      <time
        className="w-20 shrink-0 pt-0.5 text-[12.5px] tabular-nums text-muted"
        dateTime={new Date(entry.at).toISOString()}
      >
        {timeLabel(entry.at)}
      </time>
      <div className="min-w-0 flex-1">
        {showDiff && hasDiff ? (
          <DiffText segments={segments} />
        ) : (
          <p className="select-text whitespace-pre-wrap text-[13.5px] leading-relaxed text-text">
            {shown}
          </p>
        )}

        {/* Meta row: timings always shown when present; controls reveal on
            hover / focus so the feed stays calm at rest. */}
        <div className="mt-1 flex items-center gap-3 text-[12px]">
          {timings && (
            <span className="tabular-nums text-muted">{timings}</span>
          )}
          {long && !showDiff && (
            <button
              type="button"
              onClick={() => setExpanded((v) => !v)}
              className="cursor-pointer font-medium text-accent outline-none hover:underline focus-visible:underline"
            >
              {expanded ? "Show less" : "Show more"}
            </button>
          )}
          <span className="ml-auto flex items-center gap-3 opacity-0 transition-opacity duration-150 group-hover:opacity-100 group-focus-within:opacity-100">
            {hasDiff && (
              <button
                type="button"
                onClick={() => setShowDiff((v) => !v)}
                className="cursor-pointer font-medium text-muted outline-none hover:text-text focus-visible:text-text hover:underline focus-visible:underline"
              >
                {showDiff ? "Hide diff" : "Diff"}
              </button>
            )}
            {raw.length > 0 && (
              <button
                type="button"
                onClick={copyRaw}
                className="cursor-pointer font-medium text-muted outline-none hover:text-text focus-visible:text-text hover:underline focus-visible:underline"
              >
                {copied ? "Copied" : "Copy raw"}
              </button>
            )}
          </span>
        </div>
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
