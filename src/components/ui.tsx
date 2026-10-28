import type { ReactNode } from "react";
import { cx } from "./lib";

export function Panel(props: { className?: string; children: ReactNode }) {
  return (
    <div className={cx("rounded-md border border-rule bg-panel shadow-sm backdrop-blur-md", props.className)}>
      {props.children}
    </div>
  );
}

export function Switch(props: { checked: boolean; disabled?: boolean; label?: string; onChange: (value: boolean) => void }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={props.checked}
      aria-label={props.label}
      disabled={props.disabled}
      onClick={() => props.onChange(!props.checked)}
      className={cx(
        "relative h-5 w-[38px] shrink-0 rounded-full border-[1.5px] transition-colors duration-200 disabled:opacity-35",
        props.checked ? "border-accent bg-accent" : "border-muted bg-transparent hover:bg-soft",
      )}
    >
      <span
        className={cx(
          "absolute top-1/2 size-3 -translate-y-1/2 rounded-full transition-[left,background-color] duration-200 ease-out-expo",
          props.checked ? "left-[20px] bg-on-accent" : "left-[3px] bg-muted",
        )}
      />
    </button>
  );
}

/** Switch with a clickable text label. */
export function Toggle(props: { checked: boolean; disabled?: boolean; label: string; onChange: (value: boolean) => void }) {
  return (
    <label className={cx("flex shrink-0 items-center gap-2 text-caption font-semibold", props.disabled ? "opacity-40" : "cursor-pointer")}>
      <Switch checked={props.checked} disabled={props.disabled} label={props.label} onChange={props.onChange} />
      <span>{props.label}</span>
    </label>
  );
}

export function Button(props: {
  onClick: () => void;
  disabled?: boolean;
  primary?: boolean;
  title?: string;
  className?: string;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={props.onClick}
      disabled={props.disabled}
      title={props.title}
      className={cx(
        "flex h-9 shrink-0 items-center justify-center gap-2 rounded-[4px] px-3.5 text-caption font-semibold transition-[opacity,background-color] disabled:opacity-45",
        props.primary
          ? "bg-action text-on-action hover:opacity-90"
          : "border border-ink bg-panel backdrop-blur-md hover:bg-soft",
        props.className,
      )}
    >
      {props.children}
    </button>
  );
}

export function IconButton(props: { label: string; onClick: () => void; boxed?: boolean; children: ReactNode }) {
  return (
    <button
      type="button"
      aria-label={props.label}
      title={props.label}
      onClick={props.onClick}
      className={cx(
        "grid size-9 shrink-0 place-items-center rounded-[4px] text-ink transition-colors hover:bg-soft",
        props.boxed && "border border-ink bg-panel backdrop-blur-md",
      )}
    >
      {props.children}
    </button>
  );
}

function Svg(props: { size?: number; children: ReactNode; className?: string }) {
  const s = props.size ?? 18;
  return (
    <svg
      width={s}
      height={s}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      className={props.className}
    >
      {props.children}
    </svg>
  );
}

export const Icon = {
  Folder: () => (
    <Svg>
      <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
    </Svg>
  ),
  Download: () => (
    <Svg>
      <path d="M12 4v11" />
      <path d="m7 10 5 5 5-5" />
      <path d="M5 20h14" />
    </Svg>
  ),
  Gear: () => (
    <Svg>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" />
    </Svg>
  ),
  Reset: () => (
    <Svg>
      <path d="M3 12a9 9 0 1 0 3-6.7" />
      <path d="M3 4v5h5" />
    </Svg>
  ),
  Close: () => (
    <Svg>
      <path d="M6 6l12 12M18 6 6 18" />
    </Svg>
  ),
  Eye: (p: { off?: boolean }) => (
    <Svg>
      <path d="M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7S2 12 2 12z" />
      <circle cx="12" cy="12" r="3" />
      {p.off && <path d="M3 3l18 18" />}
    </Svg>
  ),
  Chevron: (p: { up?: boolean }) => (
    <Svg size={16} className={cx("transition-transform", p.up && "rotate-180")}>
      <path d="m6 9 6 6 6-6" />
    </Svg>
  ),
  Warning: () => (
    <Svg size={16}>
      <path d="M12 3 2 20h20z" />
      <path d="M12 10v4M12 17h.01" />
    </Svg>
  ),
  Live: () => (
    <Svg size={16}>
      <path d="m5 12 6-7v14z" />
      <path d="M15 5v14" />
    </Svg>
  ),
};

export function Spinner() {
  return <span className="size-3.5 shrink-0 rounded-full border-2 border-current border-r-transparent animate-spin-slow" />;
}
