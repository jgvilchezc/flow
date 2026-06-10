/**
 * Joins class name fragments, dropping falsy values. A deliberately tiny
 * replacement for `clsx` — the shell only needs conditional class joining, not
 * Tailwind conflict resolution, so there is no runtime dependency to pull in.
 */
export function cn(...parts: Array<string | false | null | undefined>): string {
  return parts.filter(Boolean).join(" ");
}
