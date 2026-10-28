import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Status } from "../types";
import { cx, errorText } from "./lib";
import { Spinner } from "./ui";

const SPEEDS = [
  { level: 1, label: "Silent", icon: () => <SnailIcon /> },
  { level: 2, label: "Standard", icon: () => <CarIcon /> },
  { level: 3, label: "Sport", icon: () => <RaceCarIcon /> },
  { level: 4, label: "Ludicrous", icon: () => <RocketIcon /> },
];

type Action = "pause" | "resume" | "stop" | "light" | "speed";

/** Pause / resume / stop, chamber light and speed for the connected printer. */
export function PrinterControls(props: { status: Status }) {
  const s = props.status;
  const [pending, setPending] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirmStop, setConfirmStop] = useState(false);

  // The stop button asks once, then resets after a few seconds.
  useEffect(() => {
    if (!confirmStop) return;
    const t = setTimeout(() => setConfirmStop(false), 4000);
    return () => clearTimeout(t);
  }, [confirmStop]);

  const printing = s.gcodeState === "RUNNING" || s.gcodeState === "PREPARE";
  const paused = s.gcodeState === "PAUSE";
  const active = printing || paused;

  async function run(action: Action, value?: number, key: string = action) {
    setPending(key);
    setError(null);
    try {
      await invoke("printer_action", { action, value: value ?? null });
    } catch (e) {
      setError(errorText(e));
    } finally {
      setPending(null);
    }
  }

  const btn =
    "flex h-8 items-center justify-center gap-1.5 rounded-[4px] border border-rule px-2.5 text-caption font-semibold hover:bg-soft disabled:opacity-50";

  return (
    <div className="mt-3 space-y-2.5 border-t border-rule pt-3">
      <div className="flex gap-2">
        {active &&
          (paused ? (
            <button type="button" className={cx(btn, "flex-1")} disabled={!!pending} onClick={() => run("resume")}>
              {pending === "resume" ? <Spinner /> : <PlayIcon />}
              Resume
            </button>
          ) : (
            <button type="button" className={cx(btn, "flex-1")} disabled={!!pending} onClick={() => run("pause")}>
              {pending === "pause" ? <Spinner /> : <PauseIcon />}
              Pause
            </button>
          ))}
        {active && (
          <button
            type="button"
            className={cx(btn, "flex-1", confirmStop && "border-error bg-error text-bg hover:bg-error")}
            disabled={!!pending}
            onClick={() => {
              if (!confirmStop) return setConfirmStop(true);
              setConfirmStop(false);
              run("stop");
            }}
          >
            {pending === "stop" ? <Spinner /> : <StopIcon />}
            {confirmStop ? "Sure?" : "Stop"}
          </button>
        )}
        <button
          type="button"
          aria-pressed={!!s.lightOn}
          title="Chamber light"
          className={cx(btn, "flex-1", s.lightOn && "border-accent text-accent")}
          disabled={!!pending || s.lightOn == null}
          onClick={() => run("light", s.lightOn ? 0 : 1)}
        >
          {pending === "light" ? <Spinner /> : <BulbIcon />}
          Light
        </button>
      </div>

      {active && (
        <div className="grid grid-cols-4 gap-0.5 rounded-[4px] bg-soft p-0.5" role="group" aria-label="Print speed">
          {SPEEDS.map((sp) => {
            const on = s.speedLevel === sp.level;
            return (
              <button
                key={sp.level}
                type="button"
                title={`${sp.label} speed`}
                aria-pressed={on}
                disabled={!!pending}
                onClick={() => run("speed", sp.level, `speed${sp.level}`)}
                className={cx(
                  "flex h-12 flex-col items-center justify-center gap-0.5 rounded-[3px] text-[11px] font-semibold",
                  on ? "bg-bg text-accent shadow-sm" : "text-muted hover:text-ink",
                )}
              >
                {pending === `speed${sp.level}` ? <Spinner /> : sp.icon()}
                {sp.label}
              </button>
            );
          })}
        </div>
      )}

      {error && <p className="text-caption font-semibold text-error">{error}</p>}
    </div>
  );
}

const svg = { width: 14, height: 14, viewBox: "0 0 24 24", fill: "currentColor", "aria-hidden": true } as const;
const PauseIcon = () => (
  <svg {...svg}>
    <rect x="6" y="5" width="4" height="14" rx="1" />
    <rect x="14" y="5" width="4" height="14" rx="1" />
  </svg>
);
const PlayIcon = () => (
  <svg {...svg}>
    <path d="M7 5.5v13a1 1 0 0 0 1.5.86l11-6.5a1 1 0 0 0 0-1.72l-11-6.5A1 1 0 0 0 7 5.5z" />
  </svg>
);
const StopIcon = () => (
  <svg {...svg}>
    <rect x="6" y="6" width="12" height="12" rx="1.5" />
  </svg>
);
const BulbIcon = () => (
  <svg {...svg} fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <path d="M9 18h6M10 21h4M12 3a6 6 0 0 0-3.5 10.9c.6.4 1 1.1 1 1.8V16h5v-.3c0-.7.4-1.4 1-1.8A6 6 0 0 0 12 3z" />
  </svg>
);

const line = {
  width: 20,
  height: 20,
  viewBox: "0 0 24 24",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.8,
  strokeLinecap: "round",
  strokeLinejoin: "round",
  "aria-hidden": true,
} as const;
const SnailIcon = () => (
  <svg {...line}>
    <path d="M2 19h14.5a3.5 3.5 0 0 0 3.5-3.5V11" />
    <path d="M20 11l-1.2-3.2M20 11l1.6-2.9" />
    <circle cx="10" cy="13" r="5" />
    <path d="M10 13a1.6 1.6 0 1 1 1.6-1.6" />
  </svg>
);
const CarIcon = () => (
  <svg {...line}>
    <path d="M3 16v-3l2.2-4.2A1.5 1.5 0 0 1 6.5 8h9.2a1.5 1.5 0 0 1 1.2.6L20 13h.5a.5.5 0 0 1 .5.5V16" />
    <path d="M3 16h1.5M9.5 16h5M19.5 16H21M3 13h17" />
    <circle cx="7" cy="16.5" r="2" />
    <circle cx="17" cy="16.5" r="2" />
  </svg>
);
const RaceCarIcon = () => (
  <svg {...line}>
    <path d="M2 16.5h2.5M9.5 16.5h5M19.5 16.5H22v-2.2l-5.5-1.8-2.5-2.5H9L7 13l-5 1.3z" />
    <path d="M2 10.5h3.5V14M14 10l2.5-2.5" />
    <circle cx="7" cy="16.5" r="2" />
    <circle cx="17" cy="16.5" r="2" />
  </svg>
);
const RocketIcon = () => (
  <svg {...line}>
    <path d="M12 2.5c3 2.2 4.5 5.6 4.5 9.5L15 15.5H9L7.5 12c0-3.9 1.5-7.3 4.5-9.5z" />
    <circle cx="12" cy="9" r="1.6" />
    <path d="M7.8 13.5 5 16.5v2.5l4-2M16.2 13.5l2.8 3v2.5l-4-2M12 18v3.5" />
  </svg>
);
