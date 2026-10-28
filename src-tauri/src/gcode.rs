//! Turns Bambu/Orca G-code into extrusion segments grouped by layer.
//!
//! Layers are split on `M73 L{n}`, which is the same number the printer
//! reports as `layer_num` over MQTT. Everything before `M73 L1` (start
//! G-code, purge line) is layer 0.
//!
//! Binary output (little endian), read by `src/gcode.ts`:
//!   u32 magic "LWG2", u32 layer_count, u32 seg_count, f32 bed_w, f32 bed_h
//!   u32 color_count, u32 colors[color_count]   filament colors 0xRRGGBB per tool
//!   u32 offsets[layer_count + 1]   first segment of each layer
//!   f32 z[layer_count], f32 height[layer_count]
//!   f32 pos[seg_count * 6]         x1 y1 z1 x2 y2 z2
//!   u8  feature[seg_count]         low 4 bits: feature id, high 4 bits: tool (T0-T15)

use std::fs::File;
use std::io::{Read, Seek};
use std::path::Path;

pub const MAGIC: u32 = u32::from_le_bytes(*b"LWG2");

pub struct Parsed {
    pub bed_w: f32,
    pub bed_h: f32,
    pub colors: Vec<u32>,
    pub offsets: Vec<u32>,
    pub z: Vec<f32>,
    pub height: Vec<f32>,
    pub pos: Vec<f32>,
    pub feature: Vec<u8>,
}

fn feature_id(name: &str) -> u8 {
    match name.trim() {
        "Outer wall" => 1,
        "Inner wall" => 2,
        "Overhang wall" => 3,
        "Sparse infill" => 4,
        "Internal solid infill" | "Solid infill" => 5,
        "Top surface" | "Ironing" => 6,
        "Bottom surface" => 7,
        "Bridge" | "Internal Bridge" => 8,
        "Gap infill" => 9,
        "Support" | "Support transition" => 10,
        "Support interface" => 11,
        "Skirt" | "Brim" => 12,
        "Prime tower" => 13,
        _ => 0,
    }
}

struct Machine {
    x: f64,
    y: f64,
    z: f64,
    e: f64,
    abs_xyz: bool,
    abs_e: bool,
    layer: usize,
    feature: u8,
    tool: u8,
    pending_height: f32,
    pending_z: f32,
}

impl Default for Parsed {
    fn default() -> Self {
        Parsed {
            bed_w: DEFAULT_BED,
            bed_h: DEFAULT_BED,
            colors: Vec::new(),
            offsets: Vec::new(),
            z: Vec::new(),
            height: Vec::new(),
            pos: Vec::new(),
            feature: Vec::new(),
        }
    }
}

const DEFAULT_BED: f32 = 180.0;

/// Reads `#FFFFFF;#00AE42` (from `; filament_colour =`) as 0xRRGGBB values.
fn filament_colors(value: &str) -> Vec<u32> {
    value
        .split([';', ','])
        .filter_map(|c| {
            let hex = c.trim().trim_start_matches('#');
            let hex = hex.get(..6)?;
            u32::from_str_radix(hex, 16).ok()
        })
        .collect()
}

/// Reads `0x0,180x0,180x180,0x180` (from `; printable_area =`) as max X / max Y.
fn bed_size(value: &str) -> Option<(f32, f32)> {
    let mut w: Option<f32> = None;
    let mut h: Option<f32> = None;
    for point in value.split(',') {
        let (x, y) = point.trim().split_once('x')?;
        let (x, y): (f32, f32) = (x.trim().parse().ok()?, y.trim().parse().ok()?);
        w = Some(w.map_or(x, |w| w.max(x)));
        h = Some(h.map_or(y, |h| h.max(y)));
    }
    match (w?, h?) {
        (w, h) if w > 0.0 && h > 0.0 => Some((w, h)),
        _ => None,
    }
}

impl Parsed {
    fn push_seg(&mut self, a: (f64, f64), b: (f64, f64), z: f64, feature: u8) {
        self.pos.extend_from_slice(&[a.0 as f32, a.1 as f32, z as f32, b.0 as f32, b.1 as f32, z as f32]);
        self.feature.push(feature);
    }

    fn seg_count(&self) -> u32 {
        self.feature.len() as u32
    }

    /// Start every layer up to `layer` (layers can be skipped, never revisited).
    fn open_layer(&mut self, layer: usize, z: f32, height: f32) {
        while self.offsets.len() <= layer {
            self.offsets.push(self.seg_count());
            self.z.push(z);
            self.height.push(height);
        }
    }

    #[cfg(test)]
    pub fn layer_count(&self) -> usize {
        self.offsets.len()
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let layers = self.offsets.len() as u32;
        let segs = self.seg_count();
        let mut out = Vec::with_capacity(24 + self.offsets.len() * 12 + self.pos.len() * 4 + self.feature.len());
        out.extend_from_slice(&MAGIC.to_le_bytes());
        out.extend_from_slice(&layers.to_le_bytes());
        out.extend_from_slice(&segs.to_le_bytes());
        out.extend_from_slice(&self.bed_w.to_le_bytes());
        out.extend_from_slice(&self.bed_h.to_le_bytes());
        out.extend_from_slice(&(self.colors.len() as u32).to_le_bytes());
        for c in &self.colors {
            out.extend_from_slice(&c.to_le_bytes());
        }
        for o in &self.offsets {
            out.extend_from_slice(&o.to_le_bytes());
        }
        out.extend_from_slice(&segs.to_le_bytes());
        for v in self.z.iter().chain(&self.height).chain(&self.pos) {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&self.feature);
        out
    }
}

/// Reads the value after a parameter letter, e.g. `X12.5`.
fn param(words: &[&str], letter: char) -> Option<f64> {
    words.iter().find_map(|w| {
        let mut c = w.chars();
        if c.next()?.eq_ignore_ascii_case(&letter) { c.as_str().parse().ok() } else { None }
    })
}

pub fn parse(text: &str) -> Parsed {
    let mut p = Parsed::default();
    let mut m = Machine {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        e: 0.0,
        abs_xyz: true,
        abs_e: true,
        layer: 0,
        feature: 0,
        tool: 0,
        pending_height: 0.2,
        pending_z: 0.0,
    };
    p.open_layer(0, 0.0, 0.2);

    for raw in text.lines() {
        let line = raw.trim();
        if let Some(comment) = line.strip_prefix(';') {
            let comment = comment.trim();
            if let Some(f) = comment.strip_prefix("FEATURE:") {
                m.feature = feature_id(f);
            } else if let Some(h) = comment.strip_prefix("LAYER_HEIGHT:") {
                m.pending_height = h.trim().parse().unwrap_or(m.pending_height);
            } else if let Some(z) = comment.strip_prefix("Z_HEIGHT:") {
                m.pending_z = z.trim().parse().unwrap_or(m.pending_z);
            } else if let Some(c) = comment.strip_prefix("filament_colour =") {
                p.colors = filament_colors(c);
            } else if let Some(area) = comment.strip_prefix("printable_area =") {
                if let Some((w, h)) = bed_size(area) {
                    p.bed_w = w;
                    p.bed_h = h;
                }
            }
            continue;
        }
        let code = line.split(';').next().unwrap_or("").trim();
        if code.is_empty() {
            continue;
        }
        let words: Vec<&str> = code.split_whitespace().collect();
        let cmd = words[0].to_ascii_uppercase();
        let args = &words[1..];

        let ink = m.feature | (m.tool << 4);
        match cmd.as_str() {
            "G0" | "G1" => {
                let (nx, ny, nz) = target(&m, args);
                let de = extrusion(&mut m, args);
                if de > 0.0 && ((nx - m.x).abs() > 1e-6 || (ny - m.y).abs() > 1e-6) {
                    p.push_seg((m.x, m.y), (nx, ny), nz, ink);
                }
                m.x = nx;
                m.y = ny;
                m.z = nz;
            }
            "G2" | "G3" => {
                let (nx, ny, nz) = target(&m, args);
                let de = extrusion(&mut m, args);
                if de > 0.0 {
                    let i = param(args, 'I').unwrap_or(0.0);
                    let j = param(args, 'J').unwrap_or(0.0);
                    arc(&mut p, (m.x, m.y), (nx, ny), (m.x + i, m.y + j), cmd == "G2", nz, ink);
                }
                m.x = nx;
                m.y = ny;
                m.z = nz;
            }
            "G90" => m.abs_xyz = true,
            "G91" => m.abs_xyz = false,
            "M82" => m.abs_e = true,
            "M83" => m.abs_e = false,
            "G92" => {
                if let Some(v) = param(args, 'E') { m.e = v; }
                if let Some(v) = param(args, 'X') { m.x = v; }
                if let Some(v) = param(args, 'Y') { m.y = v; }
                if let Some(v) = param(args, 'Z') { m.z = v; }
            }
            "M73" => {
                if let Some(l) = param(args, 'L') {
                    let l = l.max(0.0) as usize;
                    if l > m.layer {
                        m.layer = l;
                        p.open_layer(l, m.pending_z, m.pending_height);
                    }
                }
            }
            // Tool change; T255 / T1000 etc. are Bambu control codes, not filaments.
            t if t.len() > 1 && t.starts_with('T') => {
                if let Ok(n) = t[1..].parse::<u8>() {
                    if n < 16 {
                        m.tool = n;
                    }
                }
            }
            _ => {}
        }
    }
    p
}

fn target(m: &Machine, args: &[&str]) -> (f64, f64, f64) {
    let get = |c: char, cur: f64| match param(args, c) {
        Some(v) if m.abs_xyz => v,
        Some(v) => cur + v,
        None => cur,
    };
    (get('X', m.x), get('Y', m.y), get('Z', m.z))
}

/// Returns how much filament this move pushes out, and updates E.
fn extrusion(m: &mut Machine, args: &[&str]) -> f64 {
    match param(args, 'E') {
        Some(v) if m.abs_e => {
            let d = v - m.e;
            m.e = v;
            d
        }
        Some(v) => v,
        None => 0.0,
    }
}

fn arc(p: &mut Parsed, from: (f64, f64), to: (f64, f64), c: (f64, f64), clockwise: bool, z: f64, feature: u8) {
    use std::f64::consts::TAU;
    let r = ((from.0 - c.0).powi(2) + (from.1 - c.1).powi(2)).sqrt();
    if r < 1e-6 {
        return;
    }
    let a0 = (from.1 - c.1).atan2(from.0 - c.0);
    let a1 = (to.1 - c.1).atan2(to.0 - c.0);
    let mut sweep = if clockwise { a0 - a1 } else { a1 - a0 };
    if sweep <= 1e-9 {
        sweep += TAU;
    }
    let steps = ((sweep * r / 0.4).ceil() as usize).clamp(2, 256);
    let dir = if clockwise { -1.0 } else { 1.0 };
    let mut prev = from;
    for s in 1..=steps {
        let next = if s == steps {
            to
        } else {
            let a = a0 + dir * sweep * s as f64 / steps as f64;
            (c.0 + r * a.cos(), c.1 + r * a.sin())
        };
        p.push_seg(prev, next, z, feature);
        prev = next;
    }
}

/// Picks the plate G-code inside a `.gcode.3mf`. `wanted` is a path such as
/// `/data/Metadata/plate_2.gcode` from the printer; otherwise the first plate wins.
fn read_3mf<R: Read + Seek>(reader: R, wanted: Option<&str>) -> Result<String, String> {
    let mut zip = zip::ZipArchive::new(reader).map_err(|e| format!("Not a valid 3MF: {e}"))?;
    let mut plates: Vec<String> = zip
        .file_names()
        .filter(|n| n.starts_with("Metadata/plate_") && n.ends_with(".gcode"))
        .map(String::from)
        .collect();
    plates.sort();
    let name = wanted
        .and_then(|w| w.rsplit('/').next())
        .and_then(|file| plates.iter().find(|p| p.ends_with(&format!("/{file}"))))
        .or(plates.first())
        .cloned()
        .ok_or("This 3MF has no sliced G-code. Slice it in Orca, then use File > Export > Export plate sliced file.")?;
    let mut text = String::new();
    zip.by_name(&name)
        .map_err(|e| e.to_string())?
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    Ok(text)
}

pub fn load_text(path: &Path, plate: Option<&str>) -> Result<String, String> {
    let file = File::open(path).map_err(|e| format!("Can't open {}: {e}", path.display()))?;
    let lower = path.to_string_lossy().to_ascii_lowercase();
    if lower.ends_with(".3mf") {
        read_3mf(file, plate)
    } else {
        let mut text = String::new();
        std::io::BufReader::new(file).read_to_string(&mut text).map_err(|e| e.to_string())?;
        Ok(text)
    }
}

pub fn load_bytes(bytes: Vec<u8>, plate: Option<&str>) -> Result<String, String> {
    if bytes.starts_with(b"PK") {
        read_3mf(std::io::Cursor::new(bytes), plate)
    } else {
        String::from_utf8(bytes).map_err(|_| "File is not text G-code.".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_layers_on_m73() {
        let g = "M83\nG1 X0 Y0 E1\nG1 X10 Y0 E1\nM73 L1\n; Z_HEIGHT: 0.2\nG1 X10 Y10 E1\nG1 X20 Y20\nM73 L2\nG1 X0 Y0 E0.5\n";
        let p = parse(g);
        assert_eq!(p.layer_count(), 3);
        assert_eq!(p.offsets, vec![0, 1, 2]);
        assert_eq!(p.seg_count(), 3);
    }

    #[test]
    fn arcs_become_segments() {
        let p = parse("M83\nG1 X10 Y0\nG3 X-10 Y0 I-10 J0 E2\n");
        assert!(p.seg_count() >= 2);
        let last = &p.pos[p.pos.len() - 6..];
        assert!((last[3] + 10.0).abs() < 1e-4 && last[4].abs() < 1e-4);
    }

    #[test]
    fn header_has_bed_size() {
        let p = parse("; printable_area = 0x0,256x0,256x250,0x250
M83
G1 X0 Y0
G1 X10 Y0 E1
M73 L1
G1 X10 Y10 E1
");
        let b = p.to_bytes();
        let u32_at = |i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
        let f32_at = |i: usize| f32::from_le_bytes(b[i..i + 4].try_into().unwrap());
        assert_eq!(u32_at(0), MAGIC);
        assert_eq!(u32_at(4), 2);
        assert_eq!(u32_at(8), 2);
        assert_eq!((f32_at(12), f32_at(16)), (256.0, 250.0));
        assert_eq!(u32_at(20), 0); // no filament colors
        // offsets[3], z[2], height[2], pos[2*6], feature[2]
        assert_eq!(u32_at(24 + 2 * 4), 2);
        assert_eq!(b.len(), 24 + 3 * 4 + 2 * 4 + 2 * 4 + 2 * 24 + 2);
    }

    #[test]
    fn colors_and_tools() {
        let p = parse("; filament_colour = #FFFFFF;#00AE42
M83
; FEATURE: Outer wall
G1 X10 Y0 E1
T1
G1 X10 Y10 E1
T1000
G1 X0 Y10 E1
");
        assert_eq!(p.colors, vec![0xFFFFFF, 0x00AE42]);
        assert_eq!(p.feature, vec![1, 1 | (1 << 4), 1 | (1 << 4)]);
        let b = p.to_bytes();
        assert_eq!(u32::from_le_bytes(b[20..24].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(b[24..28].try_into().unwrap()), 0xFFFFFF);
    }

    #[test]
    fn bed_defaults_to_180() {
        let p = parse("G1 X1 Y1
");
        assert_eq!((p.bed_w, p.bed_h), (180.0, 180.0));
    }

    #[test]
    fn real_file_if_present() {
        let Ok(path) = std::env::var("LW_SAMPLE") else { return };
        let text = load_text(Path::new(&path), None).unwrap();
        let p = parse(&text);
        println!(
            "layers={} segs={} bytes={} bed={}x{}",
            p.layer_count(),
            p.seg_count(),
            p.to_bytes().len(),
            p.bed_w,
            p.bed_h
        );
        assert!(p.layer_count() > 1);
        assert_eq!(p.offsets.len(), p.z.len());
        assert!(p.offsets.windows(2).all(|w| w[0] <= w[1]));
        let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
        for v in p.pos.chunks(3) {
            for k in 0..3 {
                min[k] = min[k].min(v[k]);
                max[k] = max[k].max(v[k]);
            }
        }
        println!("x {:.2}..{:.2}  y {:.2}..{:.2}  z {:.2}..{:.2}", min[0], max[0], min[1], max[1], min[2], max[2]);
        assert!(min[0] > -5.0 && min[1] > -5.0 && max[0] < p.bed_w + 5.0 && max[1] < p.bed_h + 5.0);
        // From layer 1 on, layer Z (from the header comment) rises and every segment sits near it.
        let mut off = 0;
        for l in 1..p.layer_count() {
            assert!(p.z[l] > p.z[l - 1] || l == 1, "layer {l} z {} after {}", p.z[l], p.z[l - 1]);
            let (a, b) = (p.offsets[l] as usize, p.offsets.get(l + 1).copied().unwrap_or(p.seg_count()) as usize);
            for s in a..b {
                let z = p.pos[s * 6 + 2];
                if (z - p.z[l]).abs() > 0.05 {
                    off += 1;
                }
            }
        }
        println!("segments off their layer z: {off}");
    }
}
