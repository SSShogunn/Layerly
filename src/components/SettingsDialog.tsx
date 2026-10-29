import { useEffect, useRef, useState, type ReactNode } from "react";
import type { Settings } from "../types";
import { invoke } from "@tauri-apps/api/core";
import { Button, Icon, IconButton, Spinner, Switch } from "./ui";

function Field(props: {
  label: string;
  value: string;
  placeholder?: string;
  type?: string;
  autoFocus?: boolean;
  onChange: (v: string) => void;
  trailing?: ReactNode;
}) {
  return (
    <label className="block">
      <span className="text-caption font-semibold">{props.label}</span>
      <div className="mt-1 flex h-10 items-center rounded-[4px] border border-rule bg-bg focus-within:border-ink">
        <input
          className="h-full min-w-0 flex-1 bg-transparent px-3 outline-none placeholder:text-muted/70"
          type={props.type ?? "text"}
          value={props.value}
          placeholder={props.placeholder}
          autoFocus={props.autoFocus}
          spellCheck={false}
          autoComplete="off"
          onChange={(e) => props.onChange(e.target.value)}
        />
        {props.trailing}
      </div>
    </label>
  );
}

export function SettingsDialog(props: {
  initial: Settings;
  onSave: (s: Settings) => Promise<void>;
  onClose: () => void;
}) {
  const [s, setS] = useState(props.initial);
  const [showCode, setShowCode] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const formRef = useRef<HTMLFormElement>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") props.onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [props.onClose]);

  async function save() {
    setSaving(true);
    setError(null);
    try {
      await props.onSave({ host: s.host.trim(), accessCode: s.accessCode.trim(), serial: s.serial.trim(), notify: s.notify });
    } catch (e) {
      setError(typeof e === "string" ? e : e instanceof Error ? e.message : String(e));
      setSaving(false);
    }
  }

  return (
    <div
      className="fixed inset-0 z-50 grid place-items-center bg-black/35 p-4"
      onMouseDown={(e) => e.target === e.currentTarget && props.onClose()}
    >
      <form
        ref={formRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="settings-title"
        onSubmit={(e) => {
          e.preventDefault();
          save();
        }}
        className="w-[400px] max-w-full rounded-md border border-rule bg-bg p-5 shadow-xl"
      >
        <div className="flex items-center justify-between">
          <h2 id="settings-title" className="text-[20px] font-extrabold">
            Printer
          </h2>
          <IconButton label="Close" onClick={props.onClose}>
            <Icon.Close />
          </IconButton>
        </div>
        <p className="mt-1 text-caption text-muted">
          Find these on the printer: Settings → LAN Only Mode (access code) and Settings → Device (serial). Layerly finds the printer on your network by itself.
        </p>

        <div className="mt-4 space-y-3">
          <Field
            label="Access code"
            value={s.accessCode}
            autoFocus
            placeholder="8 characters"
            type={showCode ? "text" : "password"}
            onChange={(accessCode) => setS({ ...s, accessCode })}
            trailing={
              <button
                type="button"
                aria-label={showCode ? "Hide access code" : "Show access code"}
                title={showCode ? "Hide" : "Show"}
                onClick={() => setShowCode(!showCode)}
                className="grid h-full w-10 shrink-0 place-items-center text-muted hover:text-ink"
              >
                <Icon.Eye off={showCode} />
              </button>
            }
          />
          <Field
            label="Serial number"
            value={s.serial}
            placeholder="0309DA…"
            onChange={(serial) => setS({ ...s, serial })}
          />
          <Field
            label="IP address (optional)"
            value={s.host}
            placeholder="Found automatically"
            onChange={(host) => setS({ ...s, host })}
          />
        </div>

        <div className="mt-5 flex items-center gap-3 border-t border-rule pt-4">
          <div className="min-w-0 flex-1">
            <div className="text-caption font-semibold">Notifications</div>
            <div className="text-caption text-muted">At 25%, 50%, 75%, first layer, and when a print finishes, pauses or fails</div>
          </div>
          <Switch label="Notifications" checked={s.notify} onChange={(notify) => setS({ ...s, notify })} />
        </div>
        <button
          type="button"
          className="mt-2 text-caption font-semibold text-accent hover:underline"
          onClick={() => invoke("test_notification").catch((e) => setError(String(e)))}
        >
          Send a test notification
        </button>

        {error && <p className="mt-3 text-caption font-semibold text-error">{error}</p>}

        <div className="mt-5 flex justify-end gap-2">
          <Button onClick={props.onClose}>Cancel</Button>
          <Button primary disabled={saving} onClick={() => formRef.current?.requestSubmit()}>
            {saving && <Spinner />}
            Save
          </Button>
        </div>
      </form>
    </div>
  );
}
