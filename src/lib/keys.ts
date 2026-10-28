// Hotkey helpers: KeyboardEvent.code → canonical tokens understood by the Rust backends.

const CODE_TO_TOKEN: Record<string, string> = {
  ControlLeft: "LeftCtrl",
  ControlRight: "RightCtrl",
  ShiftLeft: "LeftShift",
  ShiftRight: "RightShift",
  AltLeft: "LeftAlt",
  AltRight: "RightAlt",
  MetaLeft: "LeftSuper",
  MetaRight: "RightSuper",
  OSLeft: "LeftSuper",
  OSRight: "RightSuper",
  Space: "Space",
  Enter: "Enter",
  Tab: "Tab",
  Escape: "Escape",
  Backspace: "Backspace",
  Insert: "Insert",
  Delete: "Delete",
  Home: "Home",
  End: "End",
  PageUp: "PageUp",
  PageDown: "PageDown",
  ArrowUp: "Up",
  ArrowDown: "Down",
  ArrowLeft: "Left",
  ArrowRight: "Right",
  CapsLock: "CapsLock",
  ScrollLock: "ScrollLock",
  Pause: "Pause",
  PrintScreen: "PrintScreen",
  ContextMenu: "Menu",
  Backquote: "`",
  Minus: "-",
  Equal: "=",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Semicolon: ";",
  Quote: "'",
  Comma: ",",
  Period: ".",
  Slash: "/",
  NumpadEnter: "NumpadEnter",
  NumpadAdd: "Numpad+",
  NumpadSubtract: "Numpad-",
  NumpadMultiply: "Numpad*",
  NumpadDivide: "Numpad/",
  NumpadDecimal: "Numpad.",
};

export function codeToToken(code: string): string | null {
  if (CODE_TO_TOKEN[code]) return CODE_TO_TOKEN[code];
  let m = /^Key([A-Z])$/.exec(code);
  if (m) return m[1];
  m = /^Digit([0-9])$/.exec(code);
  if (m) return m[1];
  m = /^F([0-9]{1,2})$/.exec(code);
  if (m) return `F${m[1]}`;
  m = /^Numpad([0-9])$/.exec(code);
  if (m) return `Numpad${m[1]}`;
  return null;
}

const SIDED = ["LeftCtrl", "RightCtrl", "LeftShift", "RightShift", "LeftAlt", "RightAlt", "LeftSuper", "RightSuper"];
const GENERIC: Record<string, string> = {
  LeftCtrl: "Ctrl",
  RightCtrl: "Ctrl",
  LeftShift: "Shift",
  RightShift: "Shift",
  LeftAlt: "Alt",
  RightAlt: "Alt",
  LeftSuper: "Super",
  RightSuper: "Super",
};
const ORDER = ["Ctrl", "Alt", "Shift", "Super", ...SIDED];

export function isModifier(token: string): boolean {
  return SIDED.includes(token) || ["Ctrl", "Alt", "Shift", "Super"].includes(token);
}

/** Turns the set of keys pressed during recording into an accelerator string. */
export function buildAccelerator(tokens: string[]): string {
  const unique = Array.from(new Set(tokens));
  const keys = unique.filter((t) => !isModifier(t));
  const mods = unique.filter((t) => isModifier(t));
  let finalMods: string[];
  if (keys.length > 0) {
    // With a regular key, either side of a modifier works.
    finalMods = Array.from(new Set(mods.map((m) => GENERIC[m] ?? m)));
  } else {
    finalMods = mods;
  }
  finalMods.sort((a, b) => ORDER.indexOf(a) - ORDER.indexOf(b));
  return [...finalMods, ...keys.slice(-1)].join("+");
}

const PRETTY: Record<string, string> = {
  LeftCtrl: "Left Ctrl",
  RightCtrl: "Right Ctrl",
  LeftShift: "Left Shift",
  RightShift: "Right Shift",
  LeftAlt: "Left Alt",
  RightAlt: "Right Alt",
  LeftSuper: "Left Super",
  RightSuper: "Right Super",
  Mouse4: "Mouse Back",
  Mouse5: "Mouse Forward",
  MouseMiddle: "Middle Click",
  Up: "↑",
  Down: "↓",
  Left: "←",
  Right: "→",
};

export function prettyToken(token: string, os?: string): string {
  if (os === "macos") {
    const mac: Record<string, string> = { Super: "⌘", Alt: "⌥", Ctrl: "⌃", Shift: "⇧" };
    if (mac[token]) return mac[token];
  }
  return PRETTY[token] ?? token;
}

export function splitAccelerator(accel: string): string[] {
  return accel
    .split("+")
    .map((s) => s.trim())
    .filter(Boolean);
}

export function sameAccelerator(a: string, b: string): boolean {
  const x = splitAccelerator(a).sort().join("+");
  const y = splitAccelerator(b).sort().join("+");
  return x === y && x.length > 0;
}

export function isCopilotKey(accel: string): boolean {
  return sameAccelerator(accel, "Shift+Super+F23");
}
