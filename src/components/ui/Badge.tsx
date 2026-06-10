import type { HTMLAttributes } from "react";
import { cn } from "../../lib/cn";

type Variant = "default" | "accent" | "muted";

export interface BadgeProps extends HTMLAttributes<HTMLSpanElement> {
  variant?: Variant;
}

const variants: Record<Variant, string> = {
  default: "bg-bg text-text border border-border",
  accent: "bg-accent-soft text-accent",
  muted: "bg-bg text-muted border border-border",
};

export function Badge({
  variant = "default",
  className,
  ...props
}: BadgeProps) {
  return (
    <span
      className={cn(
        "inline-flex items-center rounded-full px-2 py-0.5 text-[11px] font-medium leading-none",
        variants[variant],
        className,
      )}
      {...props}
    />
  );
}
