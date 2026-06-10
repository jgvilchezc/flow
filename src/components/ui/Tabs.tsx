import { cn } from "../../lib/cn";

export interface TabItem<T extends string = string> {
  value: T;
  label: string;
}

export interface TabsProps<T extends string = string> {
  items: ReadonlyArray<TabItem<T>>;
  value: T;
  onChange: (value: T) => void;
  className?: string;
  "aria-label"?: string;
}

/**
 * Segmented control: a horizontal row of pill buttons where the active item is
 * a filled accent-soft pill. Controlled — the parent owns `value`. Keyboard
 * accessible via the native button focus order; selection is announced through
 * `aria-pressed`.
 */
export function Tabs<T extends string = string>({
  items,
  value,
  onChange,
  className,
  "aria-label": ariaLabel,
}: TabsProps<T>) {
  return (
    <div
      role="group"
      aria-label={ariaLabel}
      className={cn(
        "inline-flex items-center gap-1 rounded-[var(--radius)] border border-border bg-bg p-1",
        className,
      )}
    >
      {items.map((item) => {
        const active = item.value === value;
        return (
          <button
            key={item.value}
            type="button"
            aria-pressed={active}
            onClick={() => onChange(item.value)}
            className={cn(
              "h-8 rounded-[calc(var(--radius)-0.25rem)] px-3 text-[13px] font-medium",
              "cursor-pointer transition-colors duration-150 outline-none",
              "focus-visible:ring-2 focus-visible:ring-accent/40",
              active
                ? "bg-accent-soft text-accent"
                : "text-muted hover:text-text",
            )}
          >
            {item.label}
          </button>
        );
      })}
    </div>
  );
}
