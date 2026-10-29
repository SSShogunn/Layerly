//! Finds the printer on the local network when its IP address changes.
//!
//! Scans the /24 subnets of the last known address and of this computer for
//! port 8883, then opens TLS on each hit. A Bambu printer's certificate names
//! its serial number, so only the right printer matches.

use std::collections::BTreeSet;
use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustls::pki_types::ServerName;
use rustls::{ClientConnection, StreamOwned};

const PORT: u16 = 8883;
const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);
const TLS_TIMEOUT: Duration = Duration::from_secs(3);
const WORKERS: usize = 64;

/// Returns the printer's current IPv4 address, if it is on a nearby subnet.
pub fn find(serial: &str, last_host: &str) -> Option<String> {
    let serial = serial.trim();
    if serial.is_empty() {
        return None;
    }
    let mut prefixes = BTreeSet::new();
    if let Ok(ip) = last_host.trim().parse::<Ipv4Addr>() {
        prefixes.insert(prefix(ip));
    }
    if let Some(ip) = local_ip() {
        prefixes.insert(prefix(ip));
    }
    // Last known address first: it is the most likely to answer.
    let mut hosts: Vec<Ipv4Addr> = last_host.trim().parse().ok().into_iter().collect();
    for p in &prefixes {
        hosts.extend((1..=254).map(|n| Ipv4Addr::new(p[0], p[1], p[2], n)));
    }
    hosts.dedup();

    let queue = Arc::new(Mutex::new(hosts.into_iter()));
    let found = Arc::new(Mutex::new(None::<Ipv4Addr>));
    let workers: Vec<_> = (0..WORKERS)
        .map(|_| {
            let (queue, found, serial) = (queue.clone(), found.clone(), serial.to_string());
            std::thread::spawn(move || {
                loop {
                    if found.lock().unwrap().is_some() {
                        return;
                    }
                    let Some(ip) = queue.lock().unwrap().next() else { return };
                    if is_printer(ip, &serial) {
                        *found.lock().unwrap() = Some(ip);
                        return;
                    }
                }
            })
        })
        .collect();
    for w in workers {
        let _ = w.join();
    }
    let ip = *found.lock().unwrap();
    ip.map(|ip| ip.to_string())
}

fn prefix(ip: Ipv4Addr) -> [u8; 3] {
    let o = ip.octets();
    [o[0], o[1], o[2]]
}

/// This computer's LAN address. Connecting a UDP socket sends nothing.
fn local_ip() -> Option<Ipv4Addr> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    match sock.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}

/// True when `ip` serves TLS on 8883 with a certificate naming `serial`.
fn is_printer(ip: Ipv4Addr, serial: &str) -> bool {
    let addr = SocketAddr::new(IpAddr::V4(ip), PORT);
    let Ok(sock) = TcpStream::connect_timeout(&addr, PROBE_TIMEOUT) else { return false };
    let _ = sock.set_read_timeout(Some(TLS_TIMEOUT));
    let _ = sock.set_write_timeout(Some(TLS_TIMEOUT));
    let Ok(conn) = ClientConnection::new(Arc::new(crate::printer::tls_config()), ServerName::IpAddress(ip.into()))
    else {
        return false;
    };
    let mut tls = StreamOwned::new(conn, sock);
    while tls.conn.is_handshaking() {
        match tls.conn.complete_io(&mut tls.sock) {
            Ok(_) => {}
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    let matched = tls
        .conn
        .peer_certificates()
        .and_then(|c| c.first())
        .is_some_and(|cert| contains(cert.as_ref(), serial.as_bytes()));
    tls.conn.send_close_notify();
    let _ = tls.conn.complete_io(&mut tls.sock);
    matched
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_is_case_insensitive() {
        assert!(contains(b"..CN=0309ab..", b"0309AB"));
        assert!(!contains(b"..CN=0309ab..", b"0309AC"));
        assert!(!contains(b"abc", b""));
    }

    /// LW_SETTINGS=path/to/settings.json cargo test find_live -- --ignored --nocapture
    #[test]
    #[ignore]
    fn find_live_printer() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let path = std::env::var("LW_SETTINGS").expect("set LW_SETTINGS");
        let s = crate::settings::load(std::path::Path::new(&path));
        let start = std::time::Instant::now();
        // Start from a wrong address to prove the scan works.
        let found = find(&s.serial, "192.168.1.250");
        println!("found {found:?} in {:.1}s (saved host {})", start.elapsed().as_secs_f64(), s.host);
        assert!(found.is_some());
    }
}

