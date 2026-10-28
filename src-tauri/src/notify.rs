//! Decides when to send desktop notifications from successive printer statuses.
//!
//! The first status seen for a job only primes the tracker, so starting the app
//! mid-print doesn't replay milestones that already passed.

use crate::printer::Status;

pub const MILESTONES: [u8; 3] = [25, 50, 75];
const FIRST_LAYER: u8 = 1 << 7;

#[derive(Default, Debug)]
pub struct Tracker {
    job: Option<String>,
    state: Option<String>,
    /// Bit per milestone index, plus FIRST_LAYER.
    sent: u8,
}

#[derive(Debug, PartialEq)]
pub struct Note {
    pub title: String,
    pub body: String,
}

fn active(state: Option<&str>) -> bool {
    matches!(state, Some("RUNNING" | "PREPARE" | "PAUSE"))
}

fn passed(s: &Status) -> u8 {
    let pct = s.percent.unwrap_or(0.0);
    let mut bits = 0;
    for (i, m) in MILESTONES.iter().enumerate() {
        if pct >= *m as f64 {
            bits |= 1 << i;
        }
    }
    if s.layer.unwrap_or(0) >= 2 {
        bits |= FIRST_LAYER;
    }
    bits
}

fn time_left(s: &Status) -> String {
    match s.remaining_min {
        Some(m) if m >= 60.0 => format!("{} h {} min left", (m / 60.0).floor(), (m % 60.0).round()),
        Some(m) if m > 0.0 => format!("{} min left", m.round()),
        _ => String::new(),
    }
}

impl Tracker {
    pub fn check(&mut self, s: &Status) -> Vec<Note> {
        if s.connection != "connected" || s.gcode_state.is_none() {
            return vec![];
        }
        let state = s.gcode_state.as_deref();
        let prev = self.state.as_deref().map(str::to_string);
        let prev = prev.as_deref();
        let job = s.job_name.clone().unwrap_or_default();

        if self.job.as_deref() != Some(job.as_str()) || self.state.is_none() {
            self.job = Some(job);
            self.state = s.gcode_state.clone();
            self.sent = passed(s);
            return vec![];
        }
        self.state = s.gcode_state.clone();

        let name = if job.is_empty() { "Print".to_string() } else { job };
        let mut notes = vec![];

        // Same file printed again.
        if !active(prev) && active(state) {
            self.sent = passed(s);
            return notes;
        }

        match (prev, state) {
            (p, Some("FINISH")) if active(p) => notes.push(Note { title: "Print finished".into(), body: name.clone() }),
            (p, Some("FAILED")) if active(p) => notes.push(Note {
                title: "Print failed".into(),
                body: error_body(&name, s.print_error),
            }),
            (Some("RUNNING" | "PREPARE"), Some("PAUSE")) => notes.push(Note {
                title: "Print paused".into(),
                body: error_body(&name, s.print_error),
            }),
            _ => {}
        }

        if state == Some("RUNNING") {
            let now = passed(s);
            let new = now & !self.sent;
            if new & FIRST_LAYER != 0 {
                notes.push(Note {
                    title: "First layer done".into(),
                    body: format!("{name}: a good moment to check bed adhesion."),
                });
            }
            // One note for the highest milestone crossed, even if several were skipped.
            if let Some(m) = (0..MILESTONES.len()).rev().find(|i| new & (1 << i) != 0) {
                let left = time_left(s);
                let body = if left.is_empty() { name.clone() } else { format!("{name} · {left}") };
                notes.push(Note { title: format!("{}% printed", MILESTONES[m]), body });
            }
            self.sent |= now;
        }
        notes
    }
}

fn error_body(name: &str, code: Option<i64>) -> String {
    match code {
        Some(c) if c != 0 => format!("{name} · printer error {}", format_error(c)),
        _ => name.to_string(),
    }
}

/// Bambu shows print errors as `0300 8014`.
pub fn format_error(code: i64) -> String {
    let hex = format!("{:08X}", code as u32);
    format!("{} {}", &hex[..4], &hex[4..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(state: &str, pct: f64, layer: i64) -> Status {
        Status {
            connection: "connected",
            gcode_state: Some(state.into()),
            percent: Some(pct),
            layer: Some(layer),
            job_name: Some("Benchy".into()),
            remaining_min: Some(75.0),
            ..Status::empty()
        }
    }

    #[test]
    fn primes_without_notifying_then_fires_milestones_once() {
        let mut t = Tracker::default();
        assert!(t.check(&st("RUNNING", 30.0, 10)).is_empty()); // app opened mid-print
        assert!(t.check(&st("RUNNING", 40.0, 12)).is_empty());
        let n = t.check(&st("RUNNING", 50.0, 15));
        assert_eq!(n, vec![Note { title: "50% printed".into(), body: "Benchy · 1 h 15 min left".into() }]);
        assert!(t.check(&st("RUNNING", 51.0, 16)).is_empty());
    }

    #[test]
    fn first_layer_and_finish() {
        let mut t = Tracker::default();
        t.check(&st("PREPARE", 0.0, 0));
        assert!(t.check(&st("RUNNING", 1.0, 1)).is_empty());
        assert_eq!(t.check(&st("RUNNING", 2.0, 2))[0].title, "First layer done");
        let n = t.check(&st("FINISH", 100.0, 118));
        assert_eq!(n[0].title, "Print finished");
    }

    #[test]
    fn pause_reports_error_code() {
        let mut t = Tracker::default();
        t.check(&st("RUNNING", 10.0, 5));
        let mut s = st("PAUSE", 10.0, 5);
        s.print_error = Some(0x0300_8014);
        assert_eq!(t.check(&s)[0].body, "Benchy · printer error 0300 8014");
    }

    #[test]
    fn reprint_resets_milestones() {
        let mut t = Tracker::default();
        t.check(&st("RUNNING", 80.0, 90));
        t.check(&st("FINISH", 100.0, 118));
        assert!(t.check(&st("PREPARE", 0.0, 0)).is_empty());
        t.check(&st("RUNNING", 10.0, 3));
        assert_eq!(t.check(&st("RUNNING", 26.0, 30)).last().unwrap().title, "25% printed");
    }
}
