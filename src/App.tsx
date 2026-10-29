import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type { DownloadProgress as Progress, Model, Settings, Status } from "./types";
import { decodeModel } from "./gcode";
import { Viewer, type ColorMode } from "./viewer";
import { StatusCard } from "./components/StatusCard";
import { BottomBar } from "./components/BottomBar";
import { SettingsDialog } from "./components/SettingsDialog";
import { DownloadProgress, downloadPercent } from "./components/DownloadProgress";
import { Button, Icon, IconButton, Panel, Spinner } from "./components/ui";
import { ACTIVE_STATES, errorText, fileStem } from "./components/lib";

const OFFLINE: Status = {
  connection: "connecting", // until the backend answers
  error: null,
  gcodeState: null,
  layer: null,
  totalLayers: null,
  percent: null,
  remainingMin: null,
  jobName: null,
  gcodeFile: null,
  nozzle: null,
  nozzleTarget: null,
  bed: null,
  bedTarget: null,
  printError: null,
  speedLevel: null,
  lightOn: null,
  wifi: null,
};

const EMPTY_SETTINGS: Settings = { host: "", accessCode: "", serial: "", notify: true };

type Busy = "open" | "fetch" | null;

function clamp(v: number, lo: number, hi: number) {
  return Math.min(hi, Math.max(lo, v));
}

/** What the viewer should show when following the printer. */
function liveTarget(status: Status, model: Model | null): [number, boolean] {
  if (!model || model.layerCount === 0) return [0, true];
  const last = model.layerCount - 1;
  if (status.gcodeState === "FINISH") return [last, true];
  if (status.gcodeState && ACTIVE_STATES.includes(status.gcodeState) && status.layer != null) {
    return [clamp(status.layer, 0, last), false];
  }
  return [last, true];
}

export default function App() {
  const containerRef = useRef<HTMLDivElement>(null);
  const viewerRef = useRef<Viewer | null>(null);

  const [status, setStatus] = useState<Status>(OFFLINE);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [model, setModel] = useState<Model | null>(null);
  const [busy, setBusy] = useState<Busy>(null);
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [startedAt, setStartedAt] = useState(0);
  const [autoHint, setAutoHint] = useState<string | null>(null);
  const [live, setLive] = useState(true);
  const [scrub, setScrub] = useState(0);
  const [ghost, setGhost] = useState(true);
  const [colorMode, setColorMode] = useState<ColorMode>(() => {
    try {
      return localStorage.getItem("colorMode") === "feature" ? "feature" : "filament";
    } catch {
      return "filament";
    }
  });

  const statusRef = useRef(status);
  statusRef.current = status;
  const busyRef = useRef<Busy>(null);
  /** Job names we've already tried to download automatically. */
  const attempted = useRef(new Set<string>());

  // --- viewer lifecycle ---
  useEffect(() => {
    const viewer = new Viewer(containerRef.current!);
    viewerRef.current = viewer;
    return () => {
      viewer.dispose();
      viewerRef.current = null;
    };
  }, []);

  // --- backend wiring ---
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    listen<Status>("status", (e) => setStatus(e.payload))
      .then((fn) => (cancelled ? fn() : (unlisten = fn)))
      .catch(() => {});
    let unlistenProgress: (() => void) | undefined;
    listen<Progress>("download-progress", (e) => {
      if (busyRef.current === "fetch") setProgress(e.payload);
    })
      .then((fn) => (cancelled ? fn() : (unlistenProgress = fn)))
      .catch(() => {});
    invoke<Status>("get_status")
      .then((s) => !cancelled && setStatus(s))
      .catch(() => {});
    invoke<Settings>("get_settings")
      .then((s) => {
        if (cancelled) return;
        setSettings(s);
        if (!s.accessCode || !s.serial) setSettingsOpen(true);
      })
      .catch(() => {
        if (cancelled) return;
        setSettings(EMPTY_SETTINGS);
      });
    return () => {
      cancelled = true;
      unlisten?.();
      unlistenProgress?.();
    };
  }, []);

  const showModel = useCallback((m: Model) => {
    setModel(m);
    setLive(true);
    setError(null);
    setAutoHint(null);
  }, []);

  const fetchFromPrinter = useCallback(
    async (auto: boolean) => {
      if (busyRef.current) return;
      busyRef.current = "fetch";
      setBusy("fetch");
      setProgress(null);
      setStartedAt(performance.now());
      if (!auto) setError(null);
      const jobName = statusRef.current.jobName;
      if (jobName) attempted.current.add(jobName);
      try {
        const buf = await invoke<ArrayBuffer>("fetch_job_gcode");
        showModel(decodeModel(buf, jobName || "Printer job"));
      } catch (e) {
        if (auto) setAutoHint(errorText(e));
        else setError(errorText(e));
      } finally {
        busyRef.current = null;
        setBusy(null);
      }
    },
    [showModel],
  );

  const openFile = useCallback(async () => {
    if (busyRef.current) return;
    setError(null);
    let path: string | null;
    try {
      path = await open({
        multiple: false,
        directory: false,
        filters: [{ name: "Sliced file", extensions: ["gcode", "3mf"] }],
      });
    } catch (e) {
      setError(errorText(e));
      return;
    }
    if (!path) return;
    busyRef.current = "open";
    setBusy("open");
    // The user picked a file on purpose: don't replace it with an automatic download.
    const jobName = statusRef.current.jobName;
    if (jobName) attempted.current.add(jobName);
    try {
      const buf = await invoke<ArrayBuffer>("load_gcode", { path, plate: statusRef.current.gcodeFile });
      showModel(decodeModel(buf, fileStem(path)));
    } catch (e) {
      setError(errorText(e));
    } finally {
      busyRef.current = null;
      setBusy(null);
    }
  }, [showModel]);

  // --- auto-fetch the running job ---
  const active = !!status.gcodeState && ACTIVE_STATES.includes(status.gcodeState);
  useEffect(() => {
    if (status.connection !== "connected" || !active || !status.jobName) return;
    if (model && model.name === status.jobName) return;
    if (attempted.current.has(status.jobName) || busy) return;
    fetchFromPrinter(true);
  }, [status.connection, active, status.jobName, model, busy, fetchFromPrinter]);

  // --- layer mapping ---
  const layerCount = model?.layerCount ?? 0;
  const [liveCurrent, liveDone] = liveTarget(status, model);
  const current = live ? liveCurrent : clamp(scrub, 0, Math.max(0, layerCount - 1));
  const done = live ? liveDone : false;
  const currentRef = useRef(current);
  currentRef.current = current;

  useEffect(() => {
    viewerRef.current?.setModel(model);
  }, [model]);
  useEffect(() => {
    viewerRef.current?.setLayer(current, done);
  }, [model, current, done]);
  useEffect(() => {
    viewerRef.current?.setGhost(ghost);
  }, [ghost]);
  useEffect(() => {
    viewerRef.current?.setColorMode(colorMode);
    try {
      localStorage.setItem("colorMode", colorMode);
    } catch {
      /* storage unavailable */
    }
  }, [colorMode]);

  const scrubTo = useCallback((layer: number) => {
    setLive(false);
    setScrub(layer);
  }, []);

  const setLiveMode = useCallback((on: boolean) => {
    if (!on) setScrub(currentRef.current);
    setLive(on);
  }, []);

  // --- keyboard ---
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (settingsOpen || e.ctrlKey || e.metaKey || e.altKey) return;
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable)) return;
      if (!layerCount) return;
      if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
        e.preventDefault();
        const step = e.key === "ArrowLeft" ? -1 : 1;
        scrubTo(clamp(currentRef.current + step, 0, layerCount - 1));
      } else if (e.key === "l" || e.key === "L") {
        setLive(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [settingsOpen, layerCount, scrubTo]);

  const saveSettings = useCallback(async (s: Settings) => {
    const next = await invoke<Status>("save_settings", { settings: s });
    setSettings(s);
    setStatus(next);
    setSettingsOpen(false);
  }, []);

  const closeSettings = useCallback(() => setSettingsOpen(false), []);

  const mismatch = useMemo(() => {
    if (!model || status.connection !== "connected" || !status.totalLayers) return null;
    if (!status.gcodeState || !(ACTIVE_STATES.includes(status.gcodeState) || status.gcodeState === "FINISH")) return null;
    const fileLayers = model.layerCount - 1;
    if (status.totalLayers === fileLayers) return null;
    return `The printer reports ${status.totalLayers} layers, this file has ${fileLayers}. Is it the right file?`;
  }, [model, status.connection, status.totalLayers, status.gcodeState]);

  const canFetch = status.connection === "connected" && !!status.jobName;

  return (
    <div className="relative h-full w-full overflow-hidden bg-bg">
      <div ref={containerRef} className="absolute inset-0" />

      {!model && !busy && (
        <div className="pointer-events-none absolute inset-0 grid place-items-center p-8">
          <p className="max-w-[340px] text-center text-body font-semibold text-muted">
            Open a sliced file or start a print — the model fills in as each layer finishes.
          </p>
        </div>
      )}

      {busy && !model && (
        <div className="pointer-events-none absolute inset-0 grid place-items-center">
          {busy === "fetch" ? (
            <div className="flex flex-col items-center gap-3">
              <p className="text-body font-semibold text-muted">Downloading the job from the printer…</p>
              <DownloadProgress progress={progress} startedAt={startedAt} width="w-[300px]" />
            </div>
          ) : (
            <div className="flex items-center gap-2 text-body font-semibold text-muted">
              <Spinner />
              Reading the file…
            </div>
          )}
        </div>
      )}

      <div className="pointer-events-none absolute inset-x-4 top-4 flex items-start justify-between gap-4">
        <div className="pointer-events-auto flex flex-col gap-2">
          <StatusCard status={status} modelName={model?.name ?? null} />
          {mismatch && (
            <Panel className="flex w-[300px] max-w-[calc(100vw-32px)] items-start gap-2 border-warn bg-warn-bg p-3 text-caption font-semibold text-warn">
              <span className="mt-px shrink-0">
                <Icon.Warning />
              </span>
              {mismatch}
            </Panel>
          )}
        </div>

        <div className="pointer-events-auto flex max-w-[420px] flex-col items-end gap-2">
          <div className="flex items-center gap-2">
            <Button onClick={openFile} disabled={!!busy}>
              {busy === "open" ? <Spinner /> : <Icon.Folder />}
              Open G-code
            </Button>
            <Button
              onClick={() => fetchFromPrinter(false)}
              disabled={!!busy || !canFetch}
              title={canFetch ? "Download the current job's file from the printer" : "No job on the printer"}
            >
              {busy === "fetch" ? <Spinner /> : <Icon.Download />}
              {busy === "fetch"
                ? downloadPercent(progress) !== null
                  ? `Downloading ${downloadPercent(progress)}%`
                  : "Downloading…"
                : "Get from printer"}
            </Button>
            <IconButton
              label="Printer settings"
              boxed
              onClick={() => {
                // Re-read: the IP may have been updated after finding the printer on the network.
                invoke<Settings>("get_settings")
                  .then(setSettings)
                  .catch(() => {})
                  .finally(() => setSettingsOpen(true));
              }}
            >
              <Icon.Gear />
            </IconButton>
          </div>

          {busy === "fetch" && model && (
            <div className="rounded-[4px] bg-panel px-2.5 py-2 backdrop-blur-md">
              <DownloadProgress progress={progress} startedAt={startedAt} />
            </div>
          )}

          {model && (
            <div className="rounded-[4px] bg-panel px-2.5 py-1 text-caption backdrop-blur-md">
              <span className="font-semibold">{model.name}</span>
              <span className="text-muted"> · {Math.max(0, model.layerCount - 1)} layers</span>
            </div>
          )}

          {error && (
            <Panel className="flex items-start gap-2 border-error p-3 text-caption font-semibold text-error">
              <span className="min-w-0 flex-1">{error}</span>
              <button type="button" aria-label="Dismiss" className="shrink-0 hover:opacity-70" onClick={() => setError(null)}>
                <Icon.Close />
              </button>
            </Panel>
          )}

          {autoHint && !error && (
            <Panel className="flex items-start gap-2 p-3 text-caption">
              <div className="min-w-0 flex-1">
                <div className="font-semibold">Couldn't download the job from the printer — open the sliced file instead</div>
                <div className="mt-0.5 text-muted">{autoHint}</div>
              </div>
              <button type="button" aria-label="Dismiss" className="shrink-0 text-muted hover:text-ink" onClick={() => setAutoHint(null)}>
                <Icon.Close />
              </button>
            </Panel>
          )}
        </div>
      </div>

      <BottomBar
        layerCount={layerCount}
        value={current}
        live={live}
        ghost={ghost}
        colorMode={colorMode}
        filaments={model?.colors ?? []}
        onColorMode={setColorMode}
        onScrub={scrubTo}
        onLive={setLiveMode}
        onGhost={setGhost}
        onResetView={() => viewerRef.current?.resetCamera()}
      />

      {settingsOpen && settings && (
        <SettingsDialog initial={settings} onSave={saveSettings} onClose={closeSettings} />
      )}
    </div>
  );
}
