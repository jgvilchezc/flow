import type { ButtonHTMLAttributes } from "react";
import { cn } from "../../lib/cn";

type Variant = "default" | "ghost" | "danger";
type Size = "sm" | "md";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: Size;
}

const base =
  "inline-flex items-center justify-center gap-2 rounded-[var(--radius)] " +
  "font-medium whitespace-nowrap select-none cursor-pointer " +
  "transition-colors duration-150 outline-none " +
  "focus-visible:ring-2 focus-visible:ring-accent/40 focus-visible:ring-offset-1 " +
  "focus-visible:ring-offset-surface " +
  "disabled:opacity-50 disabled:cursor-not-allowed disabled:pointer-events-none";

const variants: Record<Variant, string> = {
  default: "bg-accent text-white hover:bg-accent/90 active:bg-accent/80",
  ghost:
    "bg-transparent text-text border border-border hover:bg-bg active:bg-accent-soft",
  danger:
    "bg-transparent text-red-600 border border-red-200 hover:bg-red-50 active:bg-red-100",
};

const sizes: Record<Size, string> = {
  sm: "h-8 px-3 text-[13px]",
  md: "h-10 px-4 text-sm",
};

export function Button({
  variant = "default",
  size = "md",
  className,
  type = "button",
  ...props
}: ButtonProps) {
  return (
    <button
      type={type}
      className={cn(base, variants[variant], sizes[size], className)}
      {...props}
    />
  );
}
