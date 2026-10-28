export function cx(...classes: (string | false | null | undefined)[]) {
  return classes.filter(Boolean).join(" ");
}

const darkQuery = window.matchMedia("(prefers-color-scheme: dark)");

function paint() {
  document.documentElement.dataset.theme = darkQuery.matches ? "dark" : "light";
}

/** Follows the system light/dark setting. */
export function applySystemTheme() {
  paint();
  darkQuery.addEventListener("change", paint);
}

export function formatRemaining(min: number | null) {
  if (min == null || min < 0) return null;
  const h = Math.floor(min / 60);
  const m = Math.round(min % 60);
  if (h === 0) return `${m} min`;
  if (m === 0) return `${h} h`;
  return `${h} h ${m} min`;
}

/** Wall-clock finish time, e.g. "14:05" or "Tue 09:30" when not today. */
export function finishTime(min: number | null, now = new Date()) {
  if (min == null || min <= 0) return null;
  const end = new Date(now.getTime() + min * 60_000);
  const time = end.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  return end.toDateString() === now.toDateString() ? time : `${end.toLocaleDateString([], { weekday: "short" })} ${time}`;
}

/** Bambu shows print errors as "0300 8014". */
export function formatPrintError(code: number) {
  const hex = (code >>> 0).toString(16).toUpperCase().padStart(8, "0");
  return `${hex.slice(0, 4)} ${hex.slice(4)}`;
}

export function formatTemp(now: number | null, target: number | null) {
  if (now == null && target == null) return "–";
  const a = now == null ? "–" : Math.round(now);
  const b = target == null ? "–" : Math.round(target);
  return `${a} / ${b} °C`;
}

const STATES: Record<string, string> = {
  IDLE: "Idle",
  PREPARE: "Preparing",
  RUNNING: "Printing",
  PAUSE: "Paused",
  FINISH: "Finished",
  FAILED: "Failed",
  SLICING: "Slicing",
};

export function stateLabel(state: string | null) {
  if (!state) return null;
  return STATES[state] ?? state.charAt(0) + state.slice(1).toLowerCase();
}

export const ACTIVE_STATES = ["RUNNING", "PREPARE", "PAUSE"];

/** "Cube_PLA.gcode.3mf" → "Cube_PLA" */
export function fileStem(path: string) {
  const base = path.split(/[\\/]/).pop() ?? path;
  return base.replace(/\.gcode\.3mf$/i, "").replace(/\.(gcode|3mf)$/i, "");
}

export function errorText(e: unknown) {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}
