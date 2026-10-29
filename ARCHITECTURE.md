# Architecture

## Layout

| Path | What it does |
|---|---|
| `src-tauri/src/printer.rs` | MQTT connection, merged printer state, `"status"` events, commands |
| `src-tauri/src/notify.rs` | Decides when to send progress / finish / pause / fail notifications |
| `src-tauri/src/ftp.rs` | Minimal FTPS client that downloads the current job |
| `src-tauri/src/gcode.rs` | Parses G-code into layered extrusion segments (binary model) |
| `src-tauri/src/settings.rs` | Printer settings stored as JSON in the app config folder |
| `src-tauri/src/lib.rs` | Tauri commands and app setup |
| `src/viewer.ts` | three.js renderer for the model |
| `src/gcode.ts` | Decodes the binary model |
| `src/App.tsx`, `src/components/` | UI |

## Tauri commands

- `get_settings() -> Settings`, `save_settings(settings) -> Status` (restarts the connection)
- `get_status() -> Status`
- `load_gcode(path, plate) -> ArrayBuffer`: local `.gcode` / `.gcode.3mf`
- `fetch_job_gcode() -> ArrayBuffer`: downloads the running job from the printer
- `printer_action(action, value)`: `pause`, `resume`, `stop`, `light` (0/1), `speed` (1-4)
- `test_notification()`

Event: `"status"` with `Status` (see `src/types.ts`), emitted on every change.

## Binary model (little endian)

```
u32 magic "LWG2", u32 layerCount, u32 segCount, f32 bedW, f32 bedH
u32 colorCount, u32 colors[colorCount]   filament colors 0xRRGGBB per tool
u32 offsets[layerCount + 1]              first segment of each layer; last = segCount
f32 z[layerCount], f32 height[layerCount]
f32 pos[segCount * 6]                    x1 y1 z1 x2 y2 z2 (mm, Z up)
u8  feature[segCount]                    low 4 bits feature id, high 4 bits tool
```

Layers split on `M73 L{n}`, the same number the printer reports as `layer_num`.
Layer 0 is everything before `M73 L1` (start G-code, purge line).

## Printer protocol (LAN-only mode)

- **MQTT**: TLS on port 8883, user `bblp`, password = access code. Subscribe to
  `device/{serial}/report`; publish to `device/{serial}/request`. On connect, send
  `pushall` to get the full state. Reports are partial `print` objects, deep-merged.
- **Certificate**: self-signed X.509 **v1**, which rustls can't parse. Verification is
  skipped, and handshake signature checks fall back when the cert can't be parsed.
- **FTPS**: implicit TLS on port 990, same credentials. The data channel must resume the
  control connection's TLS session. Jobs live at `/{name}.gcode.3mf` or `/cache/...`.
- **Not available on A1 mini**: nozzle X/Y/Z position, and `mc_print_line_number`
  (always `0`). The built-in camera on port 6000 runs at about 1 fps in LAN mode.
