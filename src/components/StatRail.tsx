import type { Stats } from "../lib/api";
import { Card } from "./ui/Card";

/** Locale-grouped integer, e.g. 12,431. */
function fmt(n: number): string {
  return Math.round(n).toLocaleString();
}

interface StatCardProps {
  value: string;
  label: string;
  hint?: string;
}

function StatCard({ value, label, hint }: StatCardProps) {
  return (
    <Card className="flex flex-col gap-1 px-5 py-4">
      <span className="text-2xl font-semibold tabular-nums tracking-tight text-text">
        {value}
      </span>
      <span className="text-[12.5px] font-medium text-muted">{label}</span>
      {hint && <span className="text-[11.5px] text-muted">{hint}</span>}
    </Card>
  );
}

export interface StatRailProps {
  stats: Stats;
}

/**
 * The compact stats row shown above the Home feed: total words, average words
 * per minute, and the current day streak. Values come straight from
 * `get_stats`, so an empty database yields a zeroed rail rather than a blank.
 */
export function StatRail({ stats }: StatRailProps) {
  return (
    <div className="grid grid-cols-3 gap-3">
      <StatCard value={fmt(stats.total_words)} label="Words dictated" />
      <StatCard value={fmt(stats.avg_wpm)} label="Avg words / min" />
      <StatCard
        value={fmt(stats.current_streak)}
        label="Day streak"
        hint={
          stats.longest_streak > 0
            ? `Longest ${fmt(stats.longest_streak)}`
            : undefined
        }
      />
    </div>
  );
}
