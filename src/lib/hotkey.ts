/** Maps accelerator tokens (tauri-plugin-global-shortcut syntax) to macOS key glyphs. */
const GLYPHS: Record<string, string> = {
  alt: "⌥",
  option: "⌥",
  cmd: "⌘",
  command: "⌘",
  super: "⌘",
  meta: "⌘",
  ctrl: "⌃",
  control: "⌃",
  shift: "⇧",
};

/** Formats an accelerator like "Alt+Space" into display parts: ["⌥", "Space"]. */
export function hotkeyParts(accelerator: string): string[] {
  return accelerator
    .split("+")
    .map((part) => part.trim())
    .filter(Boolean)
    .map((part) => GLYPHS[part.toLowerCase()] ?? part);
}
