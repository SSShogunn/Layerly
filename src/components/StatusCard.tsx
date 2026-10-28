import type { Status } from "../types";
import { cx, finishTime, formatPrintError, formatRemaining, formatTemp, stateLabel } from "./lib";
import { PrinterControls } from "./PrinterControls";
import { Panel } from "./ui";

function connectionText(s: Status) {
  switch (s.connection) {
    case "off":
      return "Not set up";
    case "connecting":
      return "Connecting…";
    case "connected":
      return "Connected";
    case "error":
      return `Can't connect: ${s.error || "unknown error"}`;
  }
}

export function StatusCard(props: { status: Status; modelName: string | null }) {
  const s = props.status;
  const connected = s.connection === "connected";
  const state = connected ? stateLabel(s.gcodeState) : null;
  const remaining = formatRemaining(s.remainingMin);
  const doneAt = s.gcodeState === "RUNNING" || s.gcodeState === "PAUSE" ? finishTime(s.remainingMin) : null;
  const job = s.jobName || props.modelName;
  const showJob = connected && s.gcodeState && s.gcodeState !== "IDLE";

  return (
    <Panel className="w-[300px] max-w-[calc(100vw-32px)] p-4">
      <div className="flex items-center gap-2 text-caption">
        <span
          className={cx(
            "size-2.5 shrink-0 rounded-full",
            s.connection === "off" && "bg-muted",
            s.connection === "connecting" && "animate-pulse-dot bg-warn",
            s.connection === "connected" && "bg-ok",
            s.connection === "error" && "bg-error",
          )}
        />
        <span className={cx("min-w-0 flex-1 font-semibold", s.connection === "error" ? "text-error" : "text-muted")}>
          {connectionText(s)}
          {s.connection === "error" && <span className="block font-normal text-muted">Retrying every 5 seconds</span>}
        </span>
        {state && (
          <span
            className={cx(
              "shrink-0 rounded-[3px] px-1.5 py-0.5 text-caption font-semibold",
              s.gcodeState === "RUNNING" && "bg-accent text-on-accent",
              s.gcodeState === "FAILED" && "bg-error text-bg",
              s.gcodeState !== "RUNNING" && s.gcodeState !== "FAILED" && "bg-soft text-ink",
            )}
          >
            {state}
          </span>
        )}
      </div>

      {showJob && job && (
        <div className="mt-3 truncate font-semibold" title={job}>
          {job}
        </div>
      )}

      {connected && s.layer != null && s.totalLayers != null && s.totalLayers > 0 && (
        <>
          <div className="mt-2 flex items-baseline gap-2">
            <span className="text-caption font-semibold text-muted">Layer</span>
            <span className="text-headline font-extrabold tracking-tight">{s.layer}</span>
            <span className="text-body font-semibold text-muted">/ {s.totalLayers}</span>
          </div>
          {s.percent != null && (
            <div className="mt-3">
              <div className="h-1 overflow-hidden rounded-full bg-rule">
                <div className="h-full bg-accent transition-[width] duration-500" style={{ width: `${Math.min(100, Math.max(0, s.percent))}%` }} />
              </div>
              <div className="mt-1.5 flex justify-between text-caption">
                <span className="font-semibold">{Math.round(s.percent)} %</span>
                {remaining && s.gcodeState !== "FINISH" && <span className="text-muted">{remaining} left</span>}
              </div>
              {doneAt && (
                <div className="mt-0.5 text-right text-caption text-muted">
                  Done at <span className="font-semibold text-ink">{doneAt}</span>
                </div>
              )}
            </div>
          )}
        </>
      )}

      {connected && !!s.printError && (
        <div className="mt-3 rounded-[4px] bg-error/10 px-2.5 py-2 text-caption font-semibold text-error">
          Printer error {formatPrintError(s.printError)}
        </div>
      )}

      {connected && (
        <div className="mt-3 grid grid-cols-2 gap-3 border-t border-rule pt-3 text-caption">
          <div>
            <div className="text-muted">Nozzle</div>
            <div className="font-semibold">{formatTemp(s.nozzle, s.nozzleTarget)}</div>
          </div>
          <div>
            <div className="text-muted">Bed</div>
            <div className="font-semibold">{formatTemp(s.bed, s.bedTarget)}</div>
          </div>
        </div>
      )}

      {connected && <PrinterControls status={s} />}
    </Panel>
  );
}
