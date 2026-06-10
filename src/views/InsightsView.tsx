import { useEffect, useState } from "react";
import { getStats, type Stats } from "../lib/api";
import { useView } from "../app/ViewContext";
import { Card, CardContent, CardHeader, CardTitle } from "../components/ui/Card";
import { EmptyState } from "../components/ui/EmptyState";
import { Spinner } from "../components/ui/Spinner";
import { Gauge } from "../components/charts/Gauge";
import { Bars, type BarDatum } from "../components/charts/Bars";
import { Heatmap } from "../components/charts/Heatmap";

const ChartIcon = (
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
    <path d="M4 19V5m0 14h16M8 19v-6m4 6V9m4 10v-8" />
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

function fmt(n: number): string {
  return Math.round(n).toLocaleString();
}

function StatCard({ value, label }: { value: string; label: string }) {
  return (
    <Card className="flex flex-col gap-1 px-5 py-4">
      <span className="text-2xl font-semibold tabular-nums tracking-tight text-text">
        {value}
      </span>
      <span className="text-[12.5px] font-medium text-muted">{label}</span>
    </Card>
  );
}

/**
 * Insights: a WPM gauge, headline counters, a per-app usage breakdown, and a
 * dictation heatmap — all hand-rolled SVG/CSS, no chart library. Refetches on
 * dataVersion. When there is no dictation history the stats come back zeroed
 * and we show an empty state rather than a wall of zeros.
 */
export function InsightsView() {
  const { dataVersion } = useView();
  const [stats, setStats] = useState<Stats>(ZERO_STATS);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    getStats()
      .then((s) => {
        if (!cancelled) setStats(s);
      })
      .catch(console.error)
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [dataVersion]);

  if (loading) {
    return (
      <div className="flex items-center justify-center px-6 py-14">
        <Spinner label="Loading insights" />
      </div>
    );
  }

  const empty = stats.total_words === 0 && stats.per_app.length === 0;
  if (empty) {
    return (
      <div className="px-6 py-6">
        <header className="mb-6">
          <h1 className="text-xl font-semibold tracking-tight text-text">
            Insights
          </h1>
        </header>
        <EmptyState
          icon={ChartIcon}
          title="No insights yet"
          hint="Once you dictate, this page fills with your words-per-minute, totals, per-app usage, and a daily heatmap."
        />
      </div>
    );
  }

  const bars: BarDatum[] = stats.per_app.map(([label, words, sessions]) => ({
    label,
    value: words,
    count: sessions,
  }));

  return (
    <div className="px-6 py-6">
      <header className="mb-6">
        <h1 className="text-xl font-semibold tracking-tight text-text">
          Insights
        </h1>
      </header>

      <div className="flex flex-col gap-4">
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
          <Card className="flex items-center justify-center px-5 py-5 sm:col-span-1">
            <Gauge value={stats.avg_wpm} />
          </Card>
          <div className="grid grid-cols-2 gap-4 sm:col-span-2">
            <StatCard value={fmt(stats.total_words)} label="Total words" />
            <StatCard value={fmt(stats.fixes_made)} label="Words corrected" />
            <StatCard
              value={fmt(stats.current_streak)}
              label="Current streak (days)"
            />
            <StatCard
              value={fmt(stats.longest_streak)}
              label="Longest streak (days)"
            />
          </div>
        </div>

        {bars.length > 0 && (
          <Card>
            <CardHeader>
              <CardTitle>Per-app usage</CardTitle>
            </CardHeader>
            <CardContent>
              <Bars data={bars} />
            </CardContent>
          </Card>
        )}

        <Card>
          <CardHeader>
            <CardTitle>Activity</CardTitle>
          </CardHeader>
          <CardContent>
            <Heatmap
              data={stats.heatmap}
              currentStreak={stats.current_streak}
              longestStreak={stats.longest_streak}
            />
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
