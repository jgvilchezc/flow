export interface BarDatum {
  label: string;
  /** Primary magnitude that drives bar width (e.g. words). */
  value: number;
  /** Secondary count shown in the right column (e.g. sessions). */
  count?: number;
}

export interface BarsProps {
  data: BarDatum[];
  /** Suffix for the value, e.g. "words". */
  valueUnit?: string;
  /** Suffix for the count, e.g. "sessions". */
  countUnit?: string;
}

function fmt(n: number): string {
  return Math.round(n).toLocaleString();
}

/** "1 word" but "2 words" — units here are plain English plurals in -s. */
function pluralize(n: number, unit: string): string {
  return Math.round(n) === 1 ? unit.replace(/s$/, "") : unit;
}

/**
 * Horizontal magnitude bars: a label, a track filled to the value's share of
 * the largest value, and the numbers on the right. Used for per-app usage. The
 * "Unknown" bucket (apps the frontmost capture missed) is rendered verbatim,
 * not hidden, so the totals stay honest.
 */
export function Bars({
  data,
  valueUnit = "words",
  countUnit = "sessions",
}: BarsProps) {
  const max = data.reduce((m, d) => Math.max(m, d.value), 0) || 1;

  return (
    <ul className="flex flex-col gap-3">
      {data.map((d) => {
        const pct = Math.max(2, Math.round((d.value / max) * 100));
        return (
          <li key={d.label} className="flex flex-col gap-1">
            <div className="flex items-baseline justify-between gap-3">
              <span className="truncate text-[13px] font-medium text-text">
                {d.label}
              </span>
              <span className="shrink-0 text-[12px] tabular-nums text-muted">
                {fmt(d.value)} {pluralize(d.value, valueUnit)}
                {d.count != null && (
                  <>
                    {" · "}
                    {fmt(d.count)} {pluralize(d.count, countUnit)}
                  </>
                )}
              </span>
            </div>
            <div
              className="h-2 overflow-hidden rounded-full bg-bg"
              role="img"
              aria-label={`${d.label}: ${fmt(d.value)} ${valueUnit}`}
            >
              <div
                className="h-full rounded-full bg-accent"
                style={{ width: `${pct}%` }}
              />
            </div>
          </li>
        );
      })}
    </ul>
  );
}
