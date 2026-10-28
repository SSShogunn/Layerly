# Layer Watch — build contract

Tauri 2 + React 19 + Vite + Tailwind 4 + three.js desktop app (Windows). Mirrors the
setup of `../audio-spil`. Shows a Bambu Lab A1 mini print in 3D, filling in layer by
layer as the printer (LAN-only mode, MQTT) reports `layer_num`.

Sample A1 mini G-code for testing (224 layers, uses G2/G3 arcs, `M73 L{n}` per layer):
`samples/a1mini-sample.gcode` (copied from Orca temp; git-ignored)

## File ownership (only edit files you own)

| Owner | Files |
|---|---|
| backend | `src-tauri/**` except `src-tauri/src/ftp.rs`; may edit `gcode.rs` |
| ftp | `src-tauri/src/ftp.rs` only (+ one dependency line in Cargo.toml) |
| viewer | `src/viewer.ts` |
| ui | `src/App.tsx`, `src/main.tsx`, `src/index.css`, `src/components/**`, `public/**` |
| shared, read-only | `src/types.ts`, `src/gcode.ts`, this file |

## Tauri commands (Rust → `invoke` from JS)

- `get_settings() -> Settings` — `{ host, accessCode, serial }` (camelCase JSON)
- `save_settings(settings: Settings) -> Status` — saves to app config dir, restarts MQTT
- `get_status() -> Status`
- `load_gcode(path: String, plate: Option<String>) -> ArrayBuffer` — `.gcode` or `.gcode.3mf`;
  returns the binary model below via `tauri::ipc::Response`. `plate` is the printer's
  `gcodeFile` (e.g. `/data/Metadata/plate_2.gcode`) or null.
- `fetch_job_gcode() -> ArrayBuffer` — downloads the current job's `.gcode.3mf` from the
  printer over FTPS and returns the same binary model. Error string if not possible.
  Backend registers it and calls `ftp::download_job(&settings, job_name) -> Result<Vec<u8>, String>`.

Errors are returned as plain `String`s fit to show to the user.

## Event

- `"status"` with payload `Status` (see `src/types.ts`), emitted on every MQTT update and
  on connection changes. `connection`: `"off" | "connecting" | "connected" | "error"`.
  `layer` = printer `layer_num` (the layer printing now, 1-based). `gcodeState` is the raw
  printer value: `IDLE | PREPARE | RUNNING | PAUSE | FINISH | FAILED | SLICING`.

## Binary model (little endian) — decoded by `src/gcode.ts`

```
u32 magic "LWG1", u32 layerCount, u32 segCount, f32 bedW, f32 bedH
u32 offsets[layerCount + 1]   first segment index of each layer; last = segCount
f32 z[layerCount], f32 height[layerCount]
f32 pos[segCount * 6]         x1 y1 z1 x2 y2 z2 (mm, G-code frame, Z up)
u8  feature[segCount]         ids listed in src/gcode.ts FEATURES
```
Layer 0 = everything before `M73 L1` (start G-code, purge line). Layer n matches printer
`layer_num` n. bedW/bedH come from `; printable_area =` (default 180 x 180).

## Viewer API (`src/viewer.ts`)

```ts
export class Viewer {
  constructor(container: HTMLElement);
  setModel(model: Model | null): void;      // frames the camera on the model
  /** Layers < current are solid; `current` is highlighted unless done; layers > current are ghosted. */
  setLayer(current: number, done: boolean): void;
  setGhost(on: boolean): void;              // show unprinted layers faintly
  resetCamera(): void;
  dispose(): void;
}
```

## Printer facts

- MQTT: TLS on `host:8883`, self-signed cert (skip verification), user `bblp`,
  password = access code. Subscribe `device/{serial}/report`. On connect publish to
  `device/{serial}/request`: `{"pushing":{"sequence_id":"0","command":"pushall","version":1,"push_target":1}}`.
  A1/P1 send partial `print` objects: deep-merge them into a stored state.
  Fields: `gcode_state, layer_num, total_layer_num, mc_percent, mc_remaining_time,
  subtask_name, gcode_file, nozzle_temper, nozzle_target_temper, bed_temper, bed_target_temper`.
- FTPS: implicit TLS on port 990, user `bblp`, password = access code, self-signed cert.
  The data channel needs TLS session reuse. Job file is usually `/{subtask_name}.gcode.3mf`
  or `/cache/{subtask_name}.gcode.3mf`.
