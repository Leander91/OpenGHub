/**
 * Macro recording, in the format the device executes.
 *
 * Steps mirror `hidpp::onboard::MacroStep`. Browser key events are mapped to
 * HID keyboard usages via `event.code`, which is layout-independent — `KeyA` is
 * the physical A key regardless of the user's keymap, and that is what the
 * device stores.
 */

export type MacroStep =
  | { step: "keyDown"; usage: number }
  | { step: "keyUp"; usage: number }
  | { step: "modifiersDown"; mask: number }
  | { step: "modifiersUp"; mask: number }
  | { step: "mouseDown"; mask: number }
  | { step: "mouseUp"; mask: number }
  | { step: "delay"; ms: number }
  // Software only: played by OpenGHub, never stored on the device.
  | { step: "text"; text: string }
  | { step: "launch"; command: string }
  | { step: "system"; value: string };

/** Whether the device can store this step (the rest need OpenGHub running). */
export function isOnboardStep(s: MacroStep): boolean {
  return s.step !== "text" && s.step !== "launch" && s.step !== "system";
}

/** G HUB's SYSTEM entries, as key names the backend understands. */
export const SYSTEM_COMMANDS: { label: string; value: string }[] = [
  { label: "Volume up", value: "XF86AudioRaiseVolume" },
  { label: "Volume down", value: "XF86AudioLowerVolume" },
  { label: "Mute", value: "XF86AudioMute" },
  { label: "Play / pause", value: "XF86AudioPlay" },
  { label: "Next track", value: "XF86AudioNext" },
  { label: "Previous track", value: "XF86AudioPrev" },
  { label: "Screenshot", value: "Print" },
  { label: "Copy", value: "ctrl+c" },
  { label: "Paste", value: "ctrl+v" },
  { label: "Cut", value: "ctrl+x" },
  { label: "Undo", value: "ctrl+z" },
  { label: "Select all", value: "ctrl+a" },
];

export type { MacroDef } from "./types";

/** `KeyboardEvent.code` → HID keyboard usage. */
const USAGES: Record<string, number> = {
  ...Object.fromEntries(
    "abcdefghijklmnopqrstuvwxyz".split("").map((c, i) => [`Key${c.toUpperCase()}`, 0x04 + i]),
  ),
  Digit1: 0x1e, Digit2: 0x1f, Digit3: 0x20, Digit4: 0x21, Digit5: 0x22,
  Digit6: 0x23, Digit7: 0x24, Digit8: 0x25, Digit9: 0x26, Digit0: 0x27,
  Enter: 0x28, Escape: 0x29, Backspace: 0x2a, Tab: 0x2b, Space: 0x2c,
  Minus: 0x2d, Equal: 0x2e, BracketLeft: 0x2f, BracketRight: 0x30,
  Backslash: 0x31, Semicolon: 0x33, Quote: 0x34, Backquote: 0x35,
  Comma: 0x36, Period: 0x37, Slash: 0x38, CapsLock: 0x39,
  ...Object.fromEntries(Array.from({ length: 12 }, (_, i) => [`F${i + 1}`, 0x3a + i])),
  PrintScreen: 0x46, ScrollLock: 0x47, Pause: 0x48, Insert: 0x49,
  Home: 0x4a, PageUp: 0x4b, Delete: 0x4c, End: 0x4d, PageDown: 0x4e,
  ArrowRight: 0x4f, ArrowLeft: 0x50, ArrowDown: 0x51, ArrowUp: 0x52,
  NumLock: 0x53, NumpadDivide: 0x54, NumpadMultiply: 0x55, NumpadSubtract: 0x56,
  NumpadAdd: 0x57, NumpadEnter: 0x58,
  ...Object.fromEntries(Array.from({ length: 9 }, (_, i) => [`Numpad${i + 1}`, 0x59 + i])),
  Numpad0: 0x62, NumpadDecimal: 0x63,
  // The ISO key beside left Shift: `< >` on Nordic and German layouts.
  IntlBackslash: 0x64, ContextMenu: 0x65,
  ...Object.fromEntries(Array.from({ length: 12 }, (_, i) => [`F${i + 13}`, 0x68 + i])),
};

/** Modifier bitmask values, matching the HID keyboard report. */
const MODIFIERS: Record<string, number> = {
  ControlLeft: 0x01,
  ShiftLeft: 0x02,
  AltLeft: 0x04,
  MetaLeft: 0x08,
  ControlRight: 0x10,
  ShiftRight: 0x20,
  AltRight: 0x40,
  MetaRight: 0x80,
};

export function usageFor(code: string): number | null {
  return USAGES[code] ?? null;
}

export function modifierFor(code: string): number | null {
  return MODIFIERS[code] ?? null;
}

const USAGE_NAMES = new Map(Object.entries(USAGES).map(([code, usage]) => [usage, code]));
const MODIFIER_NAMES = new Map(Object.entries(MODIFIERS).map(([code, mask]) => [mask, code]));

/** Short label for a step, for the editor list. */
export function describeStep(step: MacroStep): string {
  switch (step.step) {
    case "keyDown":
      return `↓ ${prettyKey(USAGE_NAMES.get(step.usage))}`;
    case "keyUp":
      return `↑ ${prettyKey(USAGE_NAMES.get(step.usage))}`;
    case "modifiersDown":
      return `↓ ${prettyKey(MODIFIER_NAMES.get(step.mask))}`;
    case "modifiersUp":
      return `↑ ${prettyKey(MODIFIER_NAMES.get(step.mask))}`;
    case "mouseDown":
      return `↓ mouse ${Math.log2(step.mask) + 1}`;
    case "mouseUp":
      return `↑ mouse ${Math.log2(step.mask) + 1}`;
    case "delay":
      return `wait ${step.ms} ms`;
    case "text":
      return `“${step.text}”`;
    case "launch":
      return `run ${step.command}`;
    case "system":
      return SYSTEM_COMMANDS.find((c) => c.value === step.value)?.label ?? step.value;
  }
}

function prettyKey(code: string | undefined): string {
  if (!code) return "?";
  if (code === "IntlBackslash") return "< >";
  return code
    .replace(/^Numpad/, "Num ")
    .replace(/^Key/, "")
    .replace(/^Digit/, "")
    .replace(/(Left|Right)$/, "")
    .replace(/^Arrow/, "");
}

/** Total run time, so the editor can show how long a macro takes. */
export function macroDuration(steps: MacroStep[]): number {
  return steps.reduce((total, s) => total + (s.step === "delay" ? s.ms : 0), 0);
}

/** Encoded size on the device, to warn before a macro overflows its sector. */
export function encodedSize(steps: MacroStep[]): number {
  const size = (s: MacroStep) =>
    !isOnboardStep(s) ? 0 : s.step === "mouseDown" || s.step === "mouseUp" || s.step === "delay" ? 3 : 2;
  return steps.reduce((total, s) => total + size(s), 0) + 1; // + terminator
}

/** Round trip a macro's timing: caps individual waits so one typo cannot hang a button. */
export const MAX_DELAY_MS = 5000;
