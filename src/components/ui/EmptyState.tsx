import type { ReactNode } from "react";
import { cn } from "../../lib/cn";

export interface EmptyStateProps {
  /** Icon slot — pass an inline SVG sized ~24px. Decorative by default. */
  icon?: ReactNode;
  title: string;
  hint?: string;
  /** Optional action (e.g. a Button) shown below the hint. */
  action?: ReactNode;
  className?: string;
}

/**
 * Centered placeholder for empty lists and not-yet-built views. Icon + title +
 * hint + optional action, on a generous vertical rhythm.
 */
export function EmptyState({
  icon,
  title,
  hint,
  action,
  className,
}: EmptyStateProps) {
  return (
    <div
      className={cn(
        "flex flex-col items-center justify-center gap-2 px-6 py-14 text-center",
        className,
      )}
    >
      {icon && (
        <span
          aria-hidden="true"
          className="mb-1 flex size-11 items-center justify-center rounded-full bg-accent-soft text-accent"
        >
          {icon}
        </span>
      )}
      <p className="text-sm font-medium text-text">{title}</p>
      {hint && <p className="max-w-xs text-[13px] leading-relaxed text-muted">{hint}</p>}
      {action && <div className="mt-2">{action}</div>}
    </div>
  );
}
