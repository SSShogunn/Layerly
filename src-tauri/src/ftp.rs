//! Minimal FTPS client for downloading the current print job from a Bambu Lab
//! printer in LAN-only mode.
//!
//! The printer runs an implicit-TLS FTP server on port 990 (user `bblp`,
//! password = access code, self-signed certificate). Its data channel only
//! accepts TLS connections that *resume* the control connection's TLS session,
//! which many FTP clients don't do. Here every connection (control + data) is
//! made from one shared `Arc<ClientConfig>` with the same `ServerName`, so
//! rustls's in-memory session cache resumes the session on the data channel
//! (TLS 1.2 session IDs/tickets and TLS 1.3 tickets alike).

use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::WebPkiSupportedAlgorithms;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, SignatureScheme, StreamOwned};

const PORT: u16 = 990;
const USER: &str = "bblp";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// How long to wait for the reply to RETR/LIST before assuming the server wants
/// the data-channel TLS handshake first.
const PRELIM_TIMEOUT: Duration = Duration::from_secs(4);
const MAX_FILE: usize = 512 * 1024 * 1024;

/// Called with (bytes received, total bytes if known) while downloading.
pub type Progress<'a> = &'a mut dyn FnMut(u64, Option<u64>);

/// Downloads the current job's `.gcode.3mf` (or `.gcode`) from the printer.
/// Blocking; run it on a blocking thread.
pub fn download_job(
    settings: &crate::settings::Settings,
    job_name: &str,
    progress: Progress,
) -> Result<Vec<u8>, String> {
    let host = settings.host.trim();
    if host.is_empty() {
        return Err("Printer IP address is not set".into());
    }
    if settings.access_code.trim().is_empty() {
        return Err("Printer access code is not set".into());
    }
    let job = job_name.trim().trim_start_matches('/');
    if job.is_empty() {
        return Err("The printer isn't reporting a print job name".into());
    }

    let mut ftp = Ftp::connect(host, settings.access_code.trim())?;
    let result = find_and_fetch(&mut ftp, job, progress);
    ftp.quit();
    result
}

fn find_and_fetch(ftp: &mut Ftp, job: &str, progress: Progress) -> Result<Vec<u8>, String> {
    let stem = strip_ext(job);
    let mut tried: Vec<String> = Vec::new();
    if stem.len() != job.len() {
        // The name already carries an extension: try it verbatim first.
        tried.push(format!("/{job}"));
        tried.push(format!("/cache/{job}"));
    }
    for p in [
        format!("/{stem}.gcode.3mf"),
        format!("/cache/{stem}.gcode.3mf"),
        format!("/{stem}.3mf"),
        format!("/{stem}.gcode"),
        format!("/cache/{stem}.gcode"),
    ] {
        if !tried.contains(&p) {
            tried.push(p);
        }
    }

    for path in &tried {
        if let Some(data) = ftp.retr(path, progress)? {
            return Ok(data);
        }
    }

    // Nothing at the usual places: list / and /cache and take the newest match.
    let prefix = stem.to_lowercase();
    let mut candidates: Vec<(String, i64)> = Vec::new();
    for dir in ["/", "/cache"] {
        let Some(entries) = ftp.list(dir)? else { continue };
        for e in entries {
            let lower = e.name.to_lowercase();
            if !e.is_dir
                && lower.starts_with(&prefix)
                && (lower.ends_with(".3mf") || lower.ends_with(".gcode"))
            {
                let path = if dir == "/" { format!("/{}", e.name) } else { format!("{dir}/{}", e.name) };
                candidates.push((path, e.time_key));
            }
        }
    }
    if candidates.len() > 1 {
        // LIST times are coarse; refine with MDTM where the server supports it.
        for c in candidates.iter_mut() {
            if let Some(t) = ftp.mdtm(&c.0) {
                c.1 = t;
            }
        }
    }
    // Prefer newest; on ties prefer .3mf over plain .gcode.
    candidates.sort_by_key(|(p, t)| (std::cmp::Reverse(*t), !p.to_lowercase().ends_with(".3mf")));
    for (path, _) in &candidates {
        if let Some(data) = ftp.retr(path, progress)? {
            return Ok(data);
        }
    }

    Err(format!(
        "Job file not found on the printer (looked for {}, and for files starting with \"{stem}\" in / and /cache)",
        tried.join(", ")
    ))
}

fn strip_ext(name: &str) -> &str {
    let lower = name.to_lowercase();
    for ext in [".gcode.3mf", ".3mf", ".gcode"] {
        if lower.ends_with(ext) {
            return &name[..name.len() - ext.len()];
        }
    }
    name
}

// ---------------------------------------------------------------------------
// FTP client

type Tls = StreamOwned<ClientConnection, TcpStream>;

struct Reply {
    code: u16,
    text: String,
}

struct Ftp {
    host: String,
    config: Arc<ClientConfig>,
    server_name: ServerName<'static>,
    ctrl: Tls,
    /// Bytes read from the control channel but not yet consumed as a full line.
    pending: Vec<u8>,
}

impl Ftp {
    fn connect(host: &str, access_code: &str) -> Result<Ftp, String> {
        let config = tls_config()?;
        let server_name = ServerName::try_from(host.to_string())
            .map_err(|_| format!("\"{host}\" isn't a valid printer address"))?;
        let sock = tcp_connect(host, PORT).map_err(|e| {
            format!("Couldn't reach the printer at {host} on port {PORT} ({}). Check the IP address and that the printer is on the same network.", io_msg(&e))
        })?;
        let conn = ClientConnection::new(config.clone(), server_name.clone())
            .map_err(|e| format!("Couldn't start a secure connection to the printer: {e}"))?;
        let mut ctrl = StreamOwned::new(conn, sock);
        while ctrl.conn.is_handshaking() {
            ctrl.conn
                .complete_io(&mut ctrl.sock)
                .map_err(|e| format!("Secure connection to the printer failed: {}", io_msg(&e)))?;
        }

        let mut ftp = Ftp { host: host.to_string(), config, server_name, ctrl, pending: Vec::new() };
        let greet = ftp.read_reply()?;
        if greet.code != 220 {
            return Err(format!("Printer FTP server refused the connection: {}", greet.text));
        }

        let r = ftp.cmd(&format!("USER {USER}"))?;
        let r = if r.code == 331 { ftp.cmd(&format!("PASS {access_code}"))? } else { r };
        match r.code {
            230 | 202 => {}
            530 | 430 => {
                return Err("Printer rejected the access code. Check it on the printer's screen (Settings → LAN Only Mode).".into());
            }
            _ => return Err(format!("Printer FTP login failed: {}", r.text)),
        }

        // Protect the data channel (required by implicit FTPS servers).
        let _ = ftp.cmd("PBSZ 0")?;
        let r = ftp.cmd("PROT P")?;
        if r.code >= 400 {
            return Err(format!("Printer refused an encrypted data channel: {}", r.text));
        }
        let r = ftp.cmd("TYPE I")?;
        if r.code >= 400 {
            return Err(format!("Printer refused binary transfer mode: {}", r.text));
        }
        Ok(ftp)
    }

    fn quit(&mut self) {
        let _ = self.ctrl.sock.set_read_timeout(Some(Duration::from_secs(2)));
        if self.send("QUIT").is_ok() {
            let _ = self.try_read_reply();
        }
        self.ctrl.conn.send_close_notify();
        let _ = self.ctrl.conn.complete_io(&mut self.ctrl.sock);
    }

    fn send(&mut self, line: &str) -> Result<(), String> {
        let mut buf = line.as_bytes().to_vec();
        buf.extend_from_slice(b"\r\n");
        self.ctrl
            .write_all(&buf)
            .and_then(|_| self.ctrl.flush())
            .map_err(|e| format!("Lost the connection to the printer: {}", io_msg(&e)))
    }

    fn cmd(&mut self, line: &str) -> Result<Reply, String> {
        self.send(line)?;
        self.read_reply()
    }

    fn read_line(&mut self) -> io::Result<String> {
        loop {
            if let Some(pos) = self.pending.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = self.pending.drain(..=pos).collect();
                let s = String::from_utf8_lossy(&line);
                return Ok(s.trim_end_matches(['\r', '\n']).to_string());
            }
            let mut buf = [0u8; 1024];
            let n = self.ctrl.read(&mut buf)?;
            if n == 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "connection closed"));
            }
            self.pending.extend_from_slice(&buf[..n]);
        }
    }

    /// Reads one (possibly multi-line) reply. Safe to retry after a timeout:
    /// partial input stays in `pending`, and a timeout can only happen before the
    /// first complete line (multi-line replies arrive together in practice).
    fn try_read_reply(&mut self) -> io::Result<Reply> {
        let first = self.read_line()?;
        let code = parse_code(&first)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, format!("bad FTP reply: {first}")))?;
        let mut text = first.clone();
        if first.as_bytes().get(3) == Some(&b'-') {
            let end = format!("{code} ");
            loop {
                let l = self.read_line()?;
                text.push('\n');
                text.push_str(&l);
                if l.starts_with(&end) || l == code.to_string() {
                    break;
                }
            }
        }
        log::trace!("ftp: <- {text}");
        Ok(Reply { code, text })
    }

    fn read_reply(&mut self) -> Result<Reply, String> {
        self.try_read_reply()
            .map_err(|e| format!("Lost the connection to the printer: {}", io_msg(&e)))
    }

    /// Opens a passive data connection (TCP only; TLS is started later).
    fn open_data(&mut self) -> Result<TcpStream, String> {
        let r = self.cmd("PASV")?;
        let port = if r.code == 227 {
            parse_pasv(&r.text)
        } else {
            let r = self.cmd("EPSV")?;
            if r.code == 229 { parse_epsv(&r.text) } else { None }
        }
        .ok_or_else(|| format!("Printer didn't open a data port: {}", r.text))?;
        // Always connect to the configured host; the PASV address may be unroutable.
        tcp_connect(&self.host, port).map_err(|e| {
            format!("Couldn't open the data connection to the printer (port {port}): {}", io_msg(&e))
        })
    }

    /// Runs a transfer command (RETR/LIST) and returns the data, or `None` if the
    /// server answered with a "file not found"-style error.
    fn transfer(&mut self, command: &str, on_data: &mut dyn FnMut(u64)) -> Result<Option<Vec<u8>>, String> {
        let sock = self.open_data()?;
        self.send(command)?;

        // Most servers reply 150 and then accept the data TLS handshake; some may want
        // the handshake first. Wait briefly for the reply, then start the handshake anyway.
        let _ = self.ctrl.sock.set_read_timeout(Some(PRELIM_TIMEOUT));
        let prelim = self.try_read_reply();
        let _ = self.ctrl.sock.set_read_timeout(Some(READ_TIMEOUT));
        let prelim = match prelim {
            Ok(r) => Some(r),
            Err(e) if is_timeout(&e) => None,
            Err(e) => return Err(format!("Lost the connection to the printer: {}", io_msg(&e))),
        };
        if let Some(r) = &prelim {
            if r.code >= 400 {
                return transfer_error(r);
            }
        }

        let conn = ClientConnection::new(self.config.clone(), self.server_name.clone())
            .map_err(|e| format!("Couldn't start a secure data connection: {e}"))?;
        let mut data = StreamOwned::new(conn, sock);
        while data.conn.is_handshaking() {
            data.conn.complete_io(&mut data.sock).map_err(|e| {
                format!("Secure data connection to the printer failed: {}", io_msg(&e))
            })?;
        }
        match data.conn.handshake_kind() {
            Some(rustls::HandshakeKind::Resumed) => log::debug!("ftp: data channel resumed the TLS session"),
            k => log::warn!("ftp: data channel did NOT resume the TLS session ({k:?})"),
        }

        let mut out = Vec::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match data.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    out.extend_from_slice(&buf[..n]);
                    on_data(out.len() as u64);
                    if out.len() > MAX_FILE {
                        return Err("The job file on the printer is unexpectedly large".into());
                    }
                }
                // Server closed TCP without close_notify: treat as end of data.
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(format!("Download from the printer failed: {}", io_msg(&e))),
            }
        }
        drop(data);

        if prelim.is_none() {
            let r = self.read_reply()?;
            if r.code >= 400 {
                return transfer_error(&r);
            }
        }
        let done = self.read_reply()?;
        if done.code >= 400 {
            return Err(format!("Download from the printer didn't complete: {}", done.text));
        }
        Ok(Some(out))
    }

    fn retr(&mut self, path: &str, progress: Progress) -> Result<Option<Vec<u8>>, String> {
        // SIZE gives the total for the progress bar, and skips missing files
        // without opening a data connection.
        let total = match self.cmd(&format!("SIZE {path}"))? {
            r if r.code == 213 => r.text.get(4..).and_then(|t| t.trim().parse::<u64>().ok()),
            r if r.code == 550 => return Ok(None),
            _ => None,
        };
        log::debug!("ftp: RETR {path} ({total:?} bytes)");
        progress(0, total);
        self.transfer(&format!("RETR {path}"), &mut |n| progress(n, total))
    }

    fn list(&mut self, dir: &str) -> Result<Option<Vec<Entry>>, String> {
        let Some(raw) = self.transfer(&format!("LIST {dir}"), &mut |_| {})? else { return Ok(None) };
        Ok(Some(String::from_utf8_lossy(&raw).lines().filter_map(parse_list_line).collect()))
    }

    fn mdtm(&mut self, path: &str) -> Option<i64> {
        let r = self.cmd(&format!("MDTM {path}")).ok()?;
        if r.code != 213 {
            return None;
        }
        let digits: String = r.text.get(4..)?.trim().chars().take(14).collect();
        digits.parse().ok()
    }
}

fn transfer_error(r: &Reply) -> Result<Option<Vec<u8>>, String> {
    match r.code {
        550 | 450 | 553 => Ok(None),
        425 | 522 => Err(format!(
            "Printer refused the data connection (TLS session reuse problem?): {}",
            r.text
        )),
        _ => Err(format!("Printer error: {}", r.text)),
    }
}

// ---------------------------------------------------------------------------
// Parsing helpers

fn parse_code(line: &str) -> Option<u16> {
    let c = line.get(..3)?;
    if c.bytes().all(|b| b.is_ascii_digit()) { c.parse().ok() } else { None }
}

/// `227 Entering Passive Mode (h1,h2,h3,h4,p1,p2)`
fn parse_pasv(text: &str) -> Option<u16> {
    let start = text.find('(')?;
    let end = text[start..].find(')')? + start;
    let nums: Vec<u16> = text[start + 1..end].split(',').filter_map(|s| s.trim().parse().ok()).collect();
    if nums.len() != 6 {
        return None;
    }
    Some(nums[4] * 256 + nums[5])
}

/// `229 Entering Extended Passive Mode (|||port|)`
fn parse_epsv(text: &str) -> Option<u16> {
    let start = text.find('(')?;
    let end = text[start..].find(')')? + start;
    text[start + 1..end].split('|').filter(|s| !s.is_empty()).last()?.parse().ok()
}

struct Entry {
    name: String,
    is_dir: bool,
    /// Sortable `YYYYMMDDHHMMSS` key, same layout as MDTM (0 if unknown).
    time_key: i64,
}

/// Parses a Unix-style `LIST` line:
/// `-rw-r--r-- 1 root root 12345 Oct 28 12:34 name with spaces.gcode.3mf`
fn parse_list_line(line: &str) -> Option<Entry> {
    let mut rest = line.trim_end();
    let mut fields = Vec::with_capacity(8);
    for _ in 0..8 {
        rest = rest.trim_start();
        let end = rest.find(char::is_whitespace)?;
        fields.push(&rest[..end]);
        rest = &rest[end..];
    }
    let name = rest.trim_start();
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    let is_dir = fields[0].starts_with('d');
    let month = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"]
        .iter()
        .position(|m| fields[5].to_lowercase().starts_with(m))
        .map(|m| m as i64 + 1)
        .unwrap_or(0);
    let day: i64 = fields[6].parse().unwrap_or(0);
    let (year, hhmm) = match fields[7].split_once(':') {
        // "HH:MM" means within the last ~6 months: sort it after year-dated entries.
        Some((h, m)) => (9999, h.parse::<i64>().unwrap_or(0) * 100 + m.parse::<i64>().unwrap_or(0)),
        None => (fields[7].parse().unwrap_or(0), 0),
    };
    let time_key = (((year * 100 + month) * 100 + day) * 10000 + hhmm) * 100;
    Some(Entry { name: name.to_string(), is_dir, time_key })
}

// ---------------------------------------------------------------------------
// Networking / TLS

fn tcp_connect(host: &str, port: u16) -> io::Result<TcpStream> {
    let mut last = io::Error::new(io::ErrorKind::NotFound, "address not found");
    for addr in (host, port).to_socket_addrs()? {
        match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
            Ok(s) => {
                s.set_read_timeout(Some(READ_TIMEOUT))?;
                s.set_write_timeout(Some(READ_TIMEOUT))?;
                let _ = s.set_nodelay(true);
                return Ok(s);
            }
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn is_timeout(e: &io::Error) -> bool {
    matches!(e.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock)
}

fn io_msg(e: &io::Error) -> String {
    if is_timeout(e) { "the printer stopped responding".into() } else { e.to_string() }
}

fn tls_config() -> Result<Arc<ClientConfig>, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = Arc::new(AcceptAnyCert(provider.signature_verification_algorithms));
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("TLS setup failed: {e}"))?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    // Default resumption: in-memory cache, TLS 1.2 session IDs + tickets, TLS 1.3 tickets.
    // The data channel reuses this same Arc'd config, so it resumes the control session.
    Ok(Arc::new(config))
}

/// The printer uses a self-signed certificate: accept any certificate, but still
/// check handshake signatures so the TLS session itself is sound.
#[derive(Debug)]
struct AcceptAnyCert(WebPkiSupportedAlgorithms);

impl ServerCertVerifier for AcceptAnyCert {
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
        lenient(rustls::crypto::verify_tls12_signature(message, cert, dss, &self.0))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        lenient(rustls::crypto::verify_tls13_signature(message, cert, dss, &self.0))
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_schemes()
    }
}

/// webpki can't parse some self-signed certs (e.g. X.509 v1). Since the certificate
/// isn't trusted anyway, don't fail the handshake over that; real signature
/// mismatches are still rejected.
pub(crate) fn lenient(r: Result<HandshakeSignatureValid, rustls::Error>) -> Result<HandshakeSignatureValid, rustls::Error> {
    match r {
        Err(rustls::Error::InvalidCertificate(rustls::CertificateError::BadSignature)) => r,
        Err(rustls::Error::InvalidCertificate(_)) => Ok(HandshakeSignatureValid::assertion()),
        r => r,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_replies() {
        assert_eq!(parse_pasv("227 Entering Passive Mode (192,168,1,5,195,80)"), Some(50000));
        assert_eq!(parse_epsv("229 Entering Extended Passive Mode (|||50001|)"), Some(50001));
        assert_eq!(strip_ext("Cube.gcode.3mf"), "Cube");
        assert_eq!(strip_ext("Cube"), "Cube");
        let e = parse_list_line("-rw-r--r--   1 root  root  12345 Oct 28 12:34 My Part v2.gcode.3mf").unwrap();
        assert_eq!(e.name, "My Part v2.gcode.3mf");
        assert!(!e.is_dir);
        assert!(parse_list_line("total 3").is_none());
    }

    /// Times each step of a real download.
    /// LW_SETTINGS=path/to/settings.json cargo test --release timing -- --ignored --nocapture
    #[test]
    #[ignore]
    fn timing_live() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let s = crate::settings::load(std::path::Path::new(&std::env::var("LW_SETTINGS").unwrap()));
        let t = std::time::Instant::now();
        let lap = |what: &str| println!("{:>6.2}s {what}", t.elapsed().as_secs_f64());
        let mut ftp = Ftp::connect(&s.host, &s.access_code).unwrap();
        lap("connected + logged in");
        let entries = ftp.list("/").unwrap().unwrap();
        lap("LIST /");
        let newest = entries.iter().filter(|e| e.name.ends_with(".3mf")).max_by_key(|e| e.time_key).unwrap();
        let path = format!("/{}", newest.name);
        let r = ftp.cmd(&format!("SIZE {path}")).unwrap();
        lap(&format!("SIZE -> {}", r.code));
        let r = ftp.cmd("SIZE /does-not-exist.3mf").unwrap();
        lap(&format!("SIZE missing -> {}", r.code));
        let miss = ftp.retr("/does-not-exist.3mf", &mut |_, _| {}).unwrap();
        lap(&format!("RETR missing -> {:?}", miss.map(|d| d.len())));
        let mut last = 0;
        let data = ftp
            .retr(&path, &mut |n, total| {
                if n >= last + 100_000 {
                    last = n;
                    println!("        {n} / {total:?}");
                }
            })
            .unwrap()
            .unwrap();
        lap(&format!("RETR {} KB", data.len() / 1024));
        let text = crate::gcode::load_bytes(data, None).unwrap();
        lap("unzip");
        let _ = crate::gcode::parse(&text).to_bytes();
        lap("parse");
    }
}
