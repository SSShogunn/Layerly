import { useState, type CSSProperties } from "react";
import { FEATURES } from "../gcode";
import type { ColorMode, RulerUnit } from "../viewer";
import { cx } from "./lib";
import { Button, Icon, IconButton, Panel, Toggle } from "./ui";

export function BottomBar(props: {
  layerCount: number;
  value: number;
  live: boolean;
  ghost: boolean;
  colorMode: ColorMode;
  filaments: string[];
  onColorMode: (mode: ColorMode) => void;
  ruler: RulerUnit;
  onRuler: (unit: RulerUnit) => void;
  onScrub: (layer: number) => void;
  onLive: (on: boolean) => void;
  onGhost: (on: boolean) => void;
  onResetView: () => void;
}) {
  const [legendOpen, setLegendOpen] = useState(false);
  const max = Math.max(0, props.layerCount - 1);
  const disabled = props.layerCount === 0;
  const fill = max > 0 ? (props.value / max) * 100 : 0;
  const byFeature = props.colorMode === "feature";
  const swatches = byFeature ? FEATURES.slice(1, 5).map((f) => f.color) : props.filaments.slice(0, 4);

  return (
    <div className="pointer-events-none absolute inset-x-4 bottom-4 flex flex-col items-end gap-2">
      {legendOpen && (
        <Panel className="pointer-events-auto w-[240px] p-3">
          <div className="mb-3 grid grid-cols-2 rounded-[4px] bg-soft p-0.5 text-caption font-semibold">
            {(["filament", "feature"] as const).map((mode) => (
              <button
                key={mode}
                type="button"
                aria-pressed={props.colorMode === mode}
                onClick={() => props.onColorMode(mode)}
                className={cx(
                  "h-7 rounded-[3px]",
                  props.colorMode === mode ? "bg-bg shadow-sm" : "text-muted hover:text-ink",
                )}
              >
                {mode === "filament" ? "Filament" : "Line types"}
              </button>
            ))}
          </div>
          {byFeature ? (
            <ul className="grid grid-cols-1 gap-1">
              {FEATURES.map((f) => (
                <li key={f.id} className="flex items-center gap-2 text-caption">
                  <span className="size-3 shrink-0 rounded-[2px] border border-black/10" style={{ background: f.color }} />
                  {f.label}
                </li>
              ))}
            </ul>
          ) : (
            <ul className="grid grid-cols-1 gap-1">
              {props.filaments.length === 0 && (
                <li className="text-caption text-muted">The file names no filament color.</li>
              )}
              {props.filaments.map((c, i) => (
                <li key={i} className="flex items-center gap-2 text-caption">
                  <span className="size-3 shrink-0 rounded-[2px] border border-black/10" style={{ background: c }} />
                  {props.filaments.length > 1 ? `Filament ${i + 1}` : "Filament"}
                  <span className="text-muted">{c.toUpperCase()}</span>
                </li>
              ))}
            </ul>
          )}
        </Panel>
      )}

      <Panel className="pointer-events-auto flex w-full flex-wrap items-center gap-x-4 gap-y-2 px-4 py-2.5">
        <div className="flex min-w-[220px] flex-1 items-center gap-3">
          <span className="w-[92px] shrink-0 text-caption">
            <span className="text-muted">Layer </span>
            <span className="font-bold">{disabled ? "–" : props.value}</span>
            {!disabled && <span className="text-muted"> / {max}</span>}
          </span>
          <input
            type="range"
            className="layer-range min-w-0 flex-1"
            aria-label="Layer"
            min={0}
            max={max}
            step={1}
            value={props.value}
            disabled={disabled}
            style={{ "--fill": `${fill}%` } as CSSProperties}
            onChange={(e) => props.onScrub(Number(e.target.value))}
          />
        </div>

        <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
          <Toggle label="Live" checked={props.live} disabled={disabled} onChange={props.onLive} />
          {!props.live && !disabled && (
            <Button onClick={() => props.onLive(true)} title="Follow the printer (L)" className="h-8">
              <Icon.Live />
              Back to live
            </Button>
          )}
          <Toggle label="Show remaining layers" checked={props.ghost} disabled={disabled} onChange={props.onGhost} />
          <div className="h-6 w-px bg-rule" />
          <div
            role="group"
            aria-label="Ruler units"
            title="Rulers on the bed"
            className="flex h-8 items-center rounded-[4px] bg-soft p-0.5 text-caption font-semibold"
          >
            <span className="px-1.5 text-muted" aria-hidden>
              <RulerIcon />
            </span>
            {(["off", "mm", "cm"] as const).map((u) => (
              <button
                key={u}
                type="button"
                aria-pressed={props.ruler === u}
                onClick={() => props.onRuler(u)}
                className={cx(
                  "h-7 rounded-[3px] px-2",
                  props.ruler === u ? "bg-bg shadow-sm" : "text-muted hover:text-ink",
                )}
              >
                {u === "off" ? "Off" : u}
              </button>
            ))}
          </div>
          <IconButton label="Reset view" onClick={props.onResetView}>
            <Icon.Reset />
          </IconButton>
          <button
            type="button"
            aria-expanded={legendOpen}
            onClick={() => setLegendOpen(!legendOpen)}
            className={cx(
              "flex h-9 items-center gap-1.5 rounded-[4px] px-2 text-caption font-semibold hover:bg-soft",
              legendOpen && "bg-soft",
            )}
          >
            <span className="flex -space-x-1">
              {swatches.map((c, i) => (
                <span key={i} className="size-2.5 rounded-full ring-2 ring-bg" style={{ background: c }} />
              ))}
            </span>
            Colors
            <Icon.Chevron up={legendOpen} />
          </button>
        </div>
      </Panel>
    </div>
  );
}

function RulerIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
      <rect x="1.5" y="5" width="13" height="6" rx="1" />
      <path d="M4.5 5v2.5M7 5v1.5M9.5 5v2.5M12 5v1.5" />
    </svg>
  );
}
