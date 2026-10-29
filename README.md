# Layerly

Watch a Bambu Lab print build up in 3D, layer by layer, live from the printer.

Layerly connects to a Bambu Lab printer in **LAN-only mode**, downloads the job that is
printing, and fills the model in on screen as each layer finishes. Built and tested on an
A1 mini; other Bambu printers that speak the same LAN protocol should work.

## Features

- **Live 3D build-up**: printed layers solid, the current layer glowing, the rest as a faint outline
- **Automatic job download** from the printer over FTPS, or open any `.gcode` / `.gcode.3mf`
- **Filament colors** from the sliced file (multi-color with AMS), or colors per line type
- **Layer slider**: scrub through any layer, then jump back to live
- **Status**: layer, progress, time left, finish time, nozzle and bed temperatures
- **Notifications** at 25%, 50%, 75%, first layer done, and when a print finishes, pauses or fails
- **Controls**: pause / resume / stop, chamber light, speed (Silent, Standard, Sport, Ludicrous)

## Setup

1. On the printer, turn on **LAN Only Mode** (Settings → LAN Only Mode).
2. Note the **IP address** and **access code** from that screen, and the **serial number**
   from Settings → Device.
3. Open Layerly and enter the three values in the settings dialog.

Your computer and the printer must be on the same network.

## Development

Requires Node.js, pnpm and a Rust toolchain.

```sh
pnpm install
pnpm tauri dev      # run the app
pnpm tauri build    # build the Windows installer
```

Tests:

```sh
cd src-tauri
cargo test
```

Live checks against a real printer are `#[ignore]`d. Point them at a settings file:

```sh
LW_SETTINGS="$APPDATA/com.sshogunn.layerly/settings.json" cargo test probe -- --ignored --nocapture
```

See [ARCHITECTURE.md](ARCHITECTURE.md) for how it works.

## Stack

Tauri 2 · Rust · React 19 · Vite · Tailwind CSS 4 · three.js
