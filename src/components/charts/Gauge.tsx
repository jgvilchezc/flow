export interface GaugeProps {
  /** The value the needle points at (e.g. average WPM). */
  value: number;
  /** Unit label shown under the value, e.g. "words / min". */
  unit?: string;
  /** Optional explicit max; defaults to max(150, value * 1.2). */
  max?: number;
}

const W = 200;
const H = 120;
const CX = W / 2;
const CY = H - 12;
const R = 84;
const STROKE = 14;

/** Polar point on the gauge arc. `t` in [0,1] maps 180°→0° (left→right). */
function point(t: number, radius: number) {
  const angle = Math.PI * (1 - t); // π (left) → 0 (right)
  return {
    x: CX + radius * Math.cos(angle),
    y: CY - radius * Math.sin(angle),
  };
}

/** SVG path for the semicircular arc from t0 to t1 along the gauge radius. */
function arcPath(t0: number, t1: number): string {
  const a = point(t0, R);
  const b = point(t1, R);
  const large = t1 - t0 > 0.5 ? 1 : 0;
  // sweep=1 draws clockwise from a to b across the top.
  return `M ${a.x.toFixed(2)} ${a.y.toFixed(2)} A ${R} ${R} 0 ${large} 1 ${b.x.toFixed(2)} ${b.y.toFixed(2)}`;
}

/**
 * A hand-rolled SVG speedometer. The track is the full semicircle; the accent
 * sweep fills from the left up to `value/max`, and a needle points at the same
 * fraction. No chart library — pure SVG arithmetic. The value is also exposed
 * as text for screen readers via the role/aria-label.
 */
export function Gauge({ value, unit = "words / min", max }: GaugeProps) {
  const top = max ?? Math.max(150, value * 1.2);
  const t = top > 0 ? Math.min(1, Math.max(0, value / top)) : 0;
  const needle = point(t, R - STROKE / 2 - 2);

  return (
    <div className="flex flex-col items-center">
      <svg
        width={W}
        height={H}
        viewBox={`0 0 ${W} ${H}`}
        role="img"
        aria-label={`${Math.round(value)} ${unit}`}
      >
        {/* Track */}
        <path
          d={arcPath(0, 1)}
          fill="none"
          stroke="var(--color-border)"
          strokeWidth={STROKE}
          strokeLinecap="round"
        />
        {/* Value sweep */}
        {t > 0 && (
          <path
            d={arcPath(0, t)}
            fill="none"
            stroke="var(--color-accent)"
            strokeWidth={STROKE}
            strokeLinecap="round"
          />
        )}
        {/* Needle */}
        <line
          x1={CX}
          y1={CY}
          x2={needle.x.toFixed(2)}
          y2={needle.y.toFixed(2)}
          stroke="var(--color-text)"
          strokeWidth={2.5}
          strokeLinecap="round"
        />
        <circle cx={CX} cy={CY} r={4} fill="var(--color-text)" />
      </svg>
      <div className="-mt-2 flex flex-col items-center">
        <span className="text-3xl font-semibold tabular-nums tracking-tight text-text">
          {Math.round(value)}
        </span>
        <span className="text-[12px] text-muted">{unit}</span>
      </div>
    </div>
  );
}
