mod ftp;
mod gcode;
mod notify;
mod printer;
mod settings;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use printer::{Printer, Status};
use settings::Settings;
use tauri::ipc::Response;
use serde_json::json;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_notification::NotificationExt;

struct AppState {
    settings_path: PathBuf,
    settings: Mutex<Settings>,
    printer: Printer,
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
fn save_settings(app: AppHandle, state: State<AppState>, settings: Settings) -> Result<Status, String> {
    let settings = settings.trimmed();
    settings::save(&state.settings_path, &settings)?;
    *state.settings.lock().unwrap() = settings.clone();
    Ok(state.printer.restart(&app, &settings))
}

#[tauri::command]
fn get_status(state: State<AppState>) -> Status {
    state.printer.status()
}

fn model_bytes(text: &str) -> Vec<u8> {
    gcode::parse(text).to_bytes()
}

/// Runs blocking work (file IO, FTP, parsing) off the async runtime.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| format!("Background task failed: {e}"))?
}

#[tauri::command]
async fn load_gcode(path: String, plate: Option<String>) -> Result<Response, String> {
    let bytes = blocking(move || {
        let text = gcode::load_text(Path::new(&path), plate.as_deref())?;
        Ok(model_bytes(&text))
    })
    .await?;
    Ok(Response::new(bytes))
}

#[tauri::command]
async fn fetch_job_gcode(state: State<'_, AppState>) -> Result<Response, String> {
    let settings = state.settings.lock().unwrap().clone();
    if !settings.is_complete() {
        return Err("Set up the printer connection first.".into());
    }
    let status = state.printer.status();
    let job = status.job_name.ok_or("The printer has no print job right now.")?;
    let plate = status.gcode_file;
    let bytes = blocking(move || {
        let file = ftp::download_job(&settings, &job)?;
        let text = gcode::load_bytes(file, plate.as_deref())?;
        Ok(model_bytes(&text))
    })
    .await?;
    Ok(Response::new(bytes))
}

/// Printer controls: pause, resume, stop, light, speed (value 1-4).
#[tauri::command]
async fn printer_action(state: State<'_, AppState>, action: String, value: Option<i64>) -> Result<(), String> {
    let payload = match action.as_str() {
        "pause" | "resume" | "stop" => json!({ "print": { "sequence_id": "0", "command": action, "param": "" } }),
        "speed" => {
            let level = value.filter(|v| (1..=4).contains(v)).ok_or("Speed must be 1-4.")?;
            json!({ "print": { "sequence_id": "0", "command": "print_speed", "param": level.to_string() } })
        }
        "light" => json!({ "system": {
            "sequence_id": "0", "command": "ledctrl", "led_node": "chamber_light",
            "led_mode": if value.unwrap_or(1) != 0 { "on" } else { "off" },
            "led_on_time": 500, "led_off_time": 500, "loop_times": 0, "interval_time": 0
        } }),
        other => return Err(format!("Unknown action: {other}")),
    };
    state.printer.send(payload).await
}

#[tauri::command]
fn test_notification(app: AppHandle) -> Result<(), String> {
    app.notification()
        .builder()
        .title("Notifications are on")
        .body("You'll hear from Layerly at 25%, 50% and 75%, and when a print finishes.")
        .show()
        .map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // rumqttc and the FTPS client use rustls without a built-in provider.
    let _ = rustls::crypto::ring::default_provider().install_default();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let dir = app.path().app_config_dir()?;
            let settings_path = settings::path(&dir);
            let settings = settings::load(&settings_path);
            app.manage(AppState { settings_path, settings: Mutex::new(settings.clone()), printer: Printer::default() });
            app.state::<AppState>().printer.restart(app.handle(), &settings);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            get_status,
            load_gcode,
            fetch_job_gcode,
            printer_action,
            test_notification
        ])
        .run(tauri::generate_context!())
        .expect("error while running Layerly");
}
