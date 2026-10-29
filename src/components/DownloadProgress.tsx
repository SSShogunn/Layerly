import type { DownloadProgress as Progress } from "../types";

function size(bytes: number): string {
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/** Percent done, or null while the total is unknown. */
export function downloadPercent(p: Progress | null): number | null {
  if (!p || !p.total) return null;
  return Math.min(100, Math.round((p.received / p.total) * 100));
}

/** Bar with "120 KB of 315 KB · 2.1 s left" under it. */
export function DownloadProgress(props: { progress: Progress | null; startedAt: number; width?: string }) {
  const p = props.progress;
  const pct = downloadPercent(p);
  const elapsed = (performance.now() - props.startedAt) / 1000;
  let detail = "Connecting to the printer…";
  if (p && p.received > 0) {
    detail = p.total ? `${size(p.received)} of ${size(p.total)}` : size(p.received);
    if (p.total && p.received >= p.total) {
      detail = "Building the 3D view…";
    } else if (p.total && elapsed > 1) {
      const rate = p.received / elapsed;
      const left = Math.ceil((p.total - p.received) / rate);
      detail += left >= 60 ? ` · ${Math.ceil(left / 60)} min left` : ` · ${left} s left`;
    }
  }
  return (
    <div className={props.width ?? "w-[260px]"}>
      <div
        className="h-1.5 overflow-hidden rounded-full bg-rule"
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={pct ?? undefined}
      >
        <div
          className={
            pct === null
              ? "h-full w-1/3 animate-pulse rounded-full bg-accent"
              : "h-full rounded-full bg-accent transition-[width] duration-150"
          }
          style={pct === null ? undefined : { width: `${pct}%` }}
        />
      </div>
      <p className="mt-1.5 text-caption font-semibold text-muted tabular-nums">{detail}</p>
    </div>
  );
}
