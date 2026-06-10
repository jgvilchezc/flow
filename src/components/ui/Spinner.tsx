import { cn } from "../../lib/cn";

export interface SpinnerProps {
  className?: string;
  label?: string;
}

/**
 * Indeterminate loading spinner. Renders an accessible label off-screen so
 * screen readers announce the loading state; sighted users see the ring.
 */
export function Spinner({ className, label = "Loading" }: SpinnerProps) {
  return (
    <span
      role="status"
      className={cn(
        "inline-block size-4 animate-spin rounded-full",
        "border-2 border-border border-t-accent",
        className,
      )}
    >
      <span className="sr-only">{label}</span>
    </span>
  );
}
