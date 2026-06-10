export interface HeatmapProps {
  /** `(day "YYYY-MM-DD", words)` pairs, any order, for up to the last year. */
  data: Array<[string, number]>;
  /** How many trailing days to render. Default ~5 months. */
  days?: number;
  currentStreak?: number;
  longestStreak?: number;
}

const WEEKDAYS = ["Mon", "", "Wed", "", "Fri", "", "Sun"];
const MONTHS = [
  "Jan",
  "Feb",
  "Mar",
  "Apr",
  "May",
  "Jun",
  "Jul",
  "Aug",
  "Sep",
  "Oct",
  "Nov",
  "Dec",
];

/** Local YYYY-MM-DD for a Date. */
function key(d: Date): string {
  const y = d.getFullYear();
  const m = `${d.getMonth() + 1}`.padStart(2, "0");
  const day = `${d.getDate()}`.padStart(2, "0");
  return `${y}-${m}-${day}`;
}

/** Tailwind background class per intensity level 0..4. */
const LEVELS = [
  "bg-bg",
  "bg-accent/25",
  "bg-accent/50",
  "bg-accent/75",
  "bg-accent",
];

/**
 * Maps a word count to a 0..4 intensity using quartile thresholds derived from
 * the non-zero values. Zero always maps to level 0 (empty).
 */
function makeLevelFn(values: number[]): (n: number) => number {
  const nonzero = values.filter((v) => v > 0).sort((a, b) => a - b);
  if (nonzero.length === 0) return () => 0;
  const q = (p: number) => nonzero[Math.floor((nonzero.length - 1) * p)];
  const t1 = q(0.25);
  const t2 = q(0.5);
  const t3 = q(0.75);
  return (n: number) => {
    if (n <= 0) return 0;
    if (n <= t1) return 1;
    if (n <= t2) return 2;
    if (n <= t3) return 3;
    return 4;
  };
}

/**
 * A GitHub-style contribution grid for dictation volume: weekday rows × week
 * columns, each cell coloured by word-count quartile. Cells carry a title
 * tooltip (date + words) and the whole chart is summarised in a text line for
 * screen readers, since colour alone is not accessible.
 */
export function Heatmap({
  data,
  days = 154,
  currentStreak,
  longestStreak,
}: HeatmapProps) {
  const counts = new Map(data.map(([d, n]) => [d, n]));

  // Build the trailing day range, then pad the start back to the previous
  // Monday so each column is a clean Mon→Sun week.
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const start = new Date(today);
  start.setDate(start.getDate() - (days - 1));
  // JS getDay(): 0=Sun..6=Sat. Shift so Monday is the first row.
  const mondayOffset = (start.getDay() + 6) % 7;
  start.setDate(start.getDate() - mondayOffset);

  const cells: Array<{ date: Date; words: number }> = [];
  for (let d = new Date(start); d <= today; d.setDate(d.getDate() + 1)) {
    const cur = new Date(d);
    cells.push({ date: cur, words: counts.get(key(cur)) ?? 0 });
  }

  const levelOf = makeLevelFn(cells.map((c) => c.words));

  // Group into week columns of 7 (Mon..Sun).
  const weeks: Array<Array<{ date: Date; words: number }>> = [];
  for (let i = 0; i < cells.length; i += 7) {
    weeks.push(cells.slice(i, i + 7));
  }

  const totalWords = cells.reduce((s, c) => s + c.words, 0);
  const activeDays = cells.filter((c) => c.words > 0).length;

  // Month labels: show a label above the first week that starts a new month.
  let lastMonth = -1;
  const monthLabels = weeks.map((week) => {
    const m = week[0].date.getMonth();
    if (m !== lastMonth) {
      lastMonth = m;
      return MONTHS[m];
    }
    return "";
  });

  return (
    <div>
      <div className="flex gap-2 overflow-x-auto">
        {/* Weekday rail */}
        <div className="flex shrink-0 flex-col gap-[3px] pt-[18px]">
          {WEEKDAYS.map((w, i) => (
            <span
              key={i}
              className="h-3 text-[9px] leading-3 text-muted"
              aria-hidden="true"
            >
              {w}
            </span>
          ))}
        </div>
        {/* Week columns */}
        <div className="flex gap-[3px]">
          {weeks.map((week, wi) => (
            <div key={wi} className="flex flex-col gap-[3px]">
              <span className="h-[15px] text-[9px] leading-[15px] text-muted">
                {monthLabels[wi]}
              </span>
              {week.map((cell) => {
                const level = levelOf(cell.words);
                const isToday = key(cell.date) === key(today);
                return (
                  <span
                    key={key(cell.date)}
                    title={`${cell.date.toLocaleDateString(undefined, {
                      month: "short",
                      day: "numeric",
                    })}: ${cell.words} word${cell.words === 1 ? "" : "s"}`}
                    className={`size-3 rounded-[3px] ${LEVELS[level]} ${
                      isToday ? "ring-1 ring-accent ring-offset-1 ring-offset-surface" : ""
                    }`}
                  />
                );
              })}
            </div>
          ))}
        </div>
      </div>

      {/* Legend + a11y summary */}
      <div className="mt-3 flex flex-wrap items-center justify-between gap-3 text-[11.5px] text-muted">
        <p>
          {activeDays} active {activeDays === 1 ? "day" : "days"} ·{" "}
          {totalWords.toLocaleString()} words in the last {weeks.length} weeks
          {currentStreak != null && longestStreak != null && (
            <>
              {" "}
              · current streak {currentStreak}, longest {longestStreak}
            </>
          )}
        </p>
        <div className="flex items-center gap-1.5" aria-hidden="true">
          <span>Less</span>
          {LEVELS.map((cls, i) => (
            <span key={i} className={`size-3 rounded-[3px] ${cls}`} />
          ))}
          <span>More</span>
        </div>
      </div>
    </div>
  );
}
