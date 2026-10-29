//! MQTT connection to a Bambu Lab printer in LAN mode.
//!
//! The printer speaks MQTT over TLS on port 8883 with a self-signed
//! certificate, so certificate checks are skipped (handshake signatures are
//! still verified). A1/P1 printers send partial `print` objects; they are
//! deep-merged into one stored state that maps to [`Status`].

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rumqttc::{
    AsyncClient, ConnectReturnCode, ConnectionError, Event, MqttOptions, Packet, QoS, TlsConfiguration, Transport,
};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use serde::Serialize;
use serde_json::Value;
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;

use crate::notify::Tracker;
use crate::settings::Settings;

const PORT: u16 = 8883;
const RETRY: Duration = Duration::from_secs(5);
const MAX_INCOMING: usize = 4 * 1024 * 1024;
const PUSHALL: &str = r#"{"pushing":{"sequence_id":"0","command":"pushall","version":1,"push_target":1}}"#;

/// Mirrors `Status` in `src/types.ts`.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub connection: &'static str,
    pub error: Option<String>,
    pub gcode_state: Option<String>,
    pub layer: Option<i64>,
    pub total_layers: Option<i64>,
    pub percent: Option<f64>,
    pub remaining_min: Option<f64>,
    pub job_name: Option<String>,
    pub gcode_file: Option<String>,
    pub nozzle: Option<f64>,
    pub nozzle_target: Option<f64>,
    pub bed: Option<f64>,
    pub bed_target: Option<f64>,
    /// Non-zero while the printer reports an error (shown as `0300 8014`).
    pub print_error: Option<i64>,
    /// 1 silent, 2 standard, 3 sport, 4 ludicrous.
    pub speed_level: Option<i64>,
    pub light_on: Option<bool>,
    pub wifi: Option<String>,
}

impl Status {
    #[cfg(test)]
    pub fn empty() -> Status {
        Inner::default().status()
    }
}

struct Inner {
    /// Merged `print` object from all reports so far.
    print: Value,
    connection: &'static str,
    error: Option<String>,
    last: Option<Status>,
    tracker: Tracker,
    notify: bool,
    client: Option<AsyncClient>,
    request_topic: String,
}

impl Default for Inner {
    fn default() -> Self {
        Inner {
            print: Value::Object(Default::default()),
            connection: "off",
            error: None,
            last: None,
            tracker: Tracker::default(),
            notify: true,
            client: None,
            request_topic: String::new(),
        }
    }
}

impl Inner {
    fn status(&self) -> Status {
        let p = &self.print;
        Status {
            connection: self.connection,
            error: self.error.clone(),
            gcode_state: text(p, "gcode_state"),
            layer: num(p, "layer_num").map(|v| v as i64),
            total_layers: num(p, "total_layer_num").map(|v| v as i64),
            percent: num(p, "mc_percent"),
            remaining_min: num(p, "mc_remaining_time"),
            job_name: text(p, "subtask_name"),
            gcode_file: text(p, "gcode_file"),
            nozzle: num(p, "nozzle_temper"),
            nozzle_target: num(p, "nozzle_target_temper"),
            bed: num(p, "bed_temper"),
            bed_target: num(p, "bed_target_temper"),
            print_error: num(p, "print_error").map(|v| v as i64),
            speed_level: num(p, "spd_lvl").map(|v| v as i64),
            light_on: p.get("lights_report").and_then(Value::as_array).and_then(|lights| {
                lights
                    .iter()
                    .find(|l| l.get("node").and_then(Value::as_str) == Some("chamber_light"))
                    .and_then(|l| l.get("mode").and_then(Value::as_str))
                    .map(|m| m != "off")
            }),
            wifi: text(p, "wifi_signal"),
        }
    }
}

/// Numbers sometimes arrive as strings (`"215.0"`).
fn num(p: &Value, key: &str) -> Option<f64> {
    let v = match p.get(key)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    };
    v.filter(|v| v.is_finite())
}

fn text(p: &Value, key: &str) -> Option<String> {
    match p.get(key)? {
        Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Merges `patch` into `base`: objects key by key, everything else replaced.
fn merge(base: &mut Value, patch: Value) {
    match (base, patch) {
        (Value::Object(b), Value::Object(p)) => {
            for (k, v) in p {
                merge(b.entry(k).or_insert(Value::Null), v);
            }
        }
        (b, p) => *b = p,
    }
}

type Shared = Arc<Mutex<Inner>>;

#[derive(Default)]
pub struct Printer {
    inner: Shared,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl Printer {
    pub fn status(&self) -> Status {
        self.inner.lock().unwrap().status()
    }

    /// Publishes a raw JSON command to the printer's request topic.
    pub async fn send(&self, payload: Value) -> Result<(), String> {
        let (client, topic) = {
            let i = self.inner.lock().unwrap();
            if i.connection != "connected" {
                return Err("The printer isn't connected.".into());
            }
            (i.client.clone().ok_or("The printer isn't connected.")?, i.request_topic.clone())
        };
        client
            .publish(topic, QoS::AtMostOnce, false, payload.to_string())
            .await
            .map_err(|e| format!("Couldn't send the command: {e}"))
    }

    /// Drops any running connection and starts a new one for `settings`.
    pub fn restart(&self, app: &AppHandle, settings: &Settings) -> Status {
        if let Some(task) = self.task.lock().unwrap().take() {
            task.abort();
        }
        let complete = settings.is_complete();
        update(app, &self.inner, |i| {
            i.print = Value::Object(Default::default());
            i.error = None;
            i.notify = settings.notify;
            i.tracker = Tracker::default();
            i.client = None;
            i.connection = if complete { "connecting" } else { "off" };
        });
        if complete {
            let task = tauri::async_runtime::spawn(run(app.clone(), self.inner.clone(), settings.clone()));
            *self.task.lock().unwrap() = Some(task);
        }
        self.status()
    }
}

/// Applies `change` and emits `"status"` if the visible status changed.
fn update(app: &AppHandle, inner: &Shared, change: impl FnOnce(&mut Inner)) {
    let (status, notes) = {
        let mut i = inner.lock().unwrap();
        change(&mut i);
        let status = i.status();
        if i.last.as_ref() == Some(&status) {
            return;
        }
        i.last = Some(status.clone());
        let notes = i.tracker.check(&status);
        (status, if i.notify { notes } else { vec![] })
    };
    let _ = app.emit("status", status);
    for n in notes {
        if let Err(e) = app.notification().builder().title(&n.title).body(&n.body).show() {
            log::warn!("notification failed: {e}");
        }
    }
}

fn client_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("layerly-{:x}-{:x}", std::process::id(), nanos & 0xffff_ffff)
}

fn options(settings: &Settings) -> MqttOptions {
    let mut o = MqttOptions::new(client_id(), settings.host.clone(), PORT);
    o.set_credentials("bblp", settings.access_code.clone());
    o.set_keep_alive(Duration::from_secs(30));
    o.set_max_packet_size(MAX_INCOMING, 64 * 1024);
    o.set_transport(Transport::tls_with_config(TlsConfiguration::Rustls(Arc::new(tls_config()))));
    o
}

async fn run(app: AppHandle, inner: Shared, settings: Settings) {
    let report = format!("device/{}/report", settings.serial);
    let request = format!("device/{}/request", settings.serial);
    loop {
        let (client, mut events) = AsyncClient::new(options(&settings), 16);
        {
            let mut i = inner.lock().unwrap();
            i.client = Some(client.clone());
            i.request_topic = request.clone();
        }
        let mut connected = false;
        let mut got_report = false;
        let error = loop {
            match events.poll().await {
                Ok(Event::Incoming(Packet::ConnAck(_))) => {
                    connected = true;
                    update(&app, &inner, |i| {
                        i.connection = "connected";
                        i.error = None;
                    });
                    let sub = client.subscribe(&report, QoS::AtMostOnce).await;
                    let push = client.publish(&request, QoS::AtMostOnce, false, PUSHALL).await;
                    if let Err(e) = sub.and(push) {
                        break format!("Couldn't talk to the printer: {e}");
                    }
                }
                Ok(Event::Incoming(Packet::Publish(msg))) => {
                    got_report = true;
                    let Ok(Value::Object(mut body)) = serde_json::from_slice::<Value>(&msg.payload) else {
                        continue;
                    };
                    if let Some(print) = body.remove("print").filter(Value::is_object) {
                        update(&app, &inner, |i| merge(&mut i.print, print));
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    log::warn!("mqtt: {e}");
                    break describe(&e, &settings.host, connected && !got_report);
                }
            }
        };
        drop(client);
        update(&app, &inner, |i| {
            i.connection = "error";
            i.error = Some(error);
        });
        // Keep showing the error while retrying; ConnAck switches to "connected".
        tokio::time::sleep(RETRY).await;
    }
}

/// A short message fit to show to the user.
fn describe(e: &ConnectionError, host: &str, dropped_before_report: bool) -> String {
    use std::io::ErrorKind;
    let dropped = || {
        if dropped_before_report {
            "Printer closed the connection (check the serial number)".to_string()
        } else {
            "Lost connection to the printer".to_string()
        }
    };
    match e {
        ConnectionError::ConnectionRefused(ConnectReturnCode::BadUserNamePassword | ConnectReturnCode::NotAuthorized) => {
            "Printer rejected the access code".into()
        }
        ConnectionError::ConnectionRefused(code) => format!("Printer refused the connection ({code:?})"),
        ConnectionError::NetworkTimeout => format!("No answer from {host} (is the printer on and in LAN mode?)"),
        ConnectionError::Tls(t) => format!("Secure connection to the printer failed: {t}"),
        ConnectionError::Io(io) => match io.kind() {
            ErrorKind::ConnectionRefused => format!("{host} refused the connection (is LAN mode on?)"),
            ErrorKind::TimedOut | ErrorKind::HostUnreachable | ErrorKind::NetworkUnreachable => {
                format!("Can't reach {host}")
            }
            _ => dropped(),
        },
        _ => dropped(),
    }
}

/// Accepts any server certificate. Handshake signatures are checked when the
/// certificate can be parsed; Bambu printers use X.509 v1 certs, which rustls cannot.
#[derive(Debug)]
struct SkipCertCheck(Arc<CryptoProvider>);

impl ServerCertVerifier for SkipCertCheck {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        crate::ftp::lenient(verify_tls12_signature(message, cert, dss, &self.0.signature_verification_algorithms))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        crate::ftp::lenient(verify_tls13_signature(message, cert, dss, &self.0.signature_verification_algorithms))
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// TLS settings for the printer's self-signed certificate.
pub fn tls_config() -> rustls::ClientConfig {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .expect("ring supports the default TLS versions")
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(SkipCertCheck(provider)))
        .with_no_client_auth()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Live check of `mc_print_line_number` against the job's G-code lines.
    #[test]
    #[ignore]
    fn line_number_probe() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let s: Settings = serde_json::from_str(&std::fs::read_to_string(std::env::var("LW_SETTINGS").unwrap()).unwrap()).unwrap();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let mut print = Value::Object(Default::default());
        rt.block_on(async {
            let (client, mut events) = AsyncClient::new(options(&s), 16);
            let start = std::time::Instant::now();
            let mut last_line = None;
            while start.elapsed() < Duration::from_secs(40) {
                if let Ok(Ok(ev)) = tokio::time::timeout(Duration::from_secs(5), events.poll()).await {
                    match ev {
                        Event::Incoming(Packet::ConnAck(_)) => {
                            client.subscribe(format!("device/{}/report", s.serial), QoS::AtMostOnce).await.unwrap();
                            client.publish(format!("device/{}/request", s.serial), QoS::AtMostOnce, false, PUSHALL).await.unwrap();
                        }
                        Event::Incoming(Packet::Publish(m)) => {
                            if let Ok(Value::Object(mut b)) = serde_json::from_slice::<Value>(&m.payload) {
                                if let Some(pr) = b.remove("print") { merge(&mut print, pr); }
                            }
                            let line = print.get("mc_print_line_number").cloned();
                            if line != last_line {
                                println!("{:>5.1}s state={} layer={} line={:?}", start.elapsed().as_secs_f64(), print["gcode_state"], print["layer_num"], line);
                                last_line = line;
                            }
                        }
                        _ => {}
                    }
                }
            }
        });
        let job = text(&print, "subtask_name").expect("job");
        let file = crate::ftp::download_job(&s, &job).unwrap();
        let gcode = crate::gcode::load_bytes(file, text(&print, "gcode_file").as_deref()).unwrap();
        let layer = num(&print, "layer_num").unwrap() as i64;
        for (i, l) in gcode.lines().enumerate() {
            if l.trim() == format!("M73 L{layer}") || l.trim() == format!("M73 L{}", layer + 1) {
                println!("file line {} (1-based): {}", i + 1, l.trim());
            }
        }
        println!("total lines {}", gcode.lines().count());
    }

    /// Live soak: counts connects/drops over 2 minutes.
    #[test]
    #[ignore]
    fn soak_live_printer() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let s: Settings = serde_json::from_str(&std::fs::read_to_string(std::env::var("LW_SETTINGS").unwrap()).unwrap()).unwrap();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let (client, mut events) = AsyncClient::new(options(&s), 16);
            let start = std::time::Instant::now();
            let mut reports = 0;
            while start.elapsed() < Duration::from_secs(120) {
                match tokio::time::timeout(Duration::from_secs(5), events.poll()).await {
                    Ok(Ok(Event::Incoming(Packet::ConnAck(_)))) => {
                        println!("{:>5.1}s CONNACK", start.elapsed().as_secs_f64());
                        client.subscribe(format!("device/{}/report", s.serial), QoS::AtMostOnce).await.unwrap();
                        client.publish(format!("device/{}/request", s.serial), QoS::AtMostOnce, false, PUSHALL).await.unwrap();
                    }
                    Ok(Ok(Event::Incoming(Packet::Publish(_)))) => reports += 1,
                    Ok(Ok(_)) => {}
                    Ok(Err(e)) => { println!("{:>5.1}s ERROR {e:?}", start.elapsed().as_secs_f64()); tokio::time::sleep(Duration::from_secs(1)).await; }
                    Err(_) => println!("{:>5.1}s quiet 5s", start.elapsed().as_secs_f64()),
                }
            }
            println!("reports={reports}");
        });
    }

    /// Live check against the real printer: `LW_SETTINGS=<settings.json> cargo test probe -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn probe_live_printer() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let path = std::env::var("LW_SETTINGS").expect("set LW_SETTINGS");
        let s: Settings = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let (client, mut events) = AsyncClient::new(options(&s), 16);
            for _ in 0..20 {
                match tokio::time::timeout(Duration::from_secs(10), events.poll()).await {
                    Ok(Ok(Event::Incoming(Packet::ConnAck(a)))) => {
                        println!("CONNACK {a:?}");
                        client.subscribe(format!("device/{}/report", s.serial), QoS::AtMostOnce).await.unwrap();
                        client.publish(format!("device/{}/request", s.serial), QoS::AtMostOnce, false, PUSHALL).await.unwrap();
                    }
                    Ok(Ok(Event::Incoming(Packet::Publish(p)))) => {
                        println!("REPORT {} bytes: {}", p.payload.len(), String::from_utf8_lossy(&p.payload[..p.payload.len().min(300)]));
                        return;
                    }
                    Ok(Ok(e)) => println!("event {e:?}"),
                    Ok(Err(e)) => { println!("ERROR {e:?}"); return; }
                    Err(_) => { println!("TIMEOUT waiting for event"); return; }
                }
            }
        });
    }

    #[test]
    fn merges_partial_reports() {
        let mut inner = Inner::default();
        merge(
            &mut inner.print,
            json!({"gcode_state": "RUNNING", "layer_num": 3, "total_layer_num": "224",
                "nozzle_temper": "219.5", "subtask_name": "cube", "upgrade_state": {"a": 1, "b": 2}}),
        );
        merge(&mut inner.print, json!({"layer_num": 4, "upgrade_state": {"b": 3}}));
        let s = inner.status();
        assert_eq!(s.layer, Some(4));
        assert_eq!(s.total_layers, Some(224));
        assert_eq!(s.nozzle, Some(219.5));
        assert_eq!(s.gcode_state.as_deref(), Some("RUNNING"));
        assert_eq!(s.job_name.as_deref(), Some("cube"));
        assert_eq!(s.bed, None);
        assert_eq!(inner.print["upgrade_state"], json!({"a": 1, "b": 3}));
    }

    #[test]
    fn status_json_matches_types_ts() {
        let v = serde_json::to_value(Inner::default().status()).unwrap();
        let keys = [
            "connection", "error", "gcodeState", "layer", "totalLayers", "percent", "remainingMin",
            "jobName", "gcodeFile", "nozzle", "nozzleTarget", "bed", "bedTarget", "printError", "speedLevel",
            "lightOn", "wifi",
        ];
        for key in keys {
            assert!(v.get(key).is_some(), "missing {key}");
        }
        assert_eq!(v.as_object().unwrap().len(), keys.len());
        assert_eq!(v["connection"], "off");
    }

    #[test]
    fn tls_config_builds() {
        let _ = tls_config();
    }
}
