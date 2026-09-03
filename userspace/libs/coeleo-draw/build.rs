//! Rasterize IBM Plex Mono into a grayscale atlas (host). The kernel never
//! sees the TTF.

use std::env;
use std::fs;
use std::path::PathBuf;

const PX: f32 = 11.0;
const PAD: i32 = 1;

fn main() {
    println!("cargo:rerun-if-changed=../../../docs/fonts/IBMPlexMono-Regular.ttf");
    let font_path = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("../../../docs/fonts/IBMPlexMono-Regular.ttf");
    let bytes =
        fs::read(&font_path).unwrap_or_else(|e| panic!("GUI font {}: {e}", font_path.display()));
    let font = fontdue::Font::from_bytes(bytes.as_slice(), fontdue::FontSettings::default())
        .unwrap_or_else(|e| panic!("parse GUI font: {e}"));

    let mut rasters: Vec<(fontdue::Metrics, Vec<u8>)> = Vec::new();
    let mut min_xmin = 0i32;
    let mut min_ymin = 0i32;
    let mut max_xmax = 1i32;
    let mut max_ymax = 1i32;
    for ch in 32u8..127 {
        let (m, bitmap) = font.rasterize(ch as char, PX);
        if m.width > 0 && m.height > 0 {
            min_xmin = min_xmin.min(m.xmin);
            min_ymin = min_ymin.min(m.ymin);
            max_xmax = max_xmax.max(m.xmin + m.width as i32);
            max_ymax = max_ymax.max(m.ymin + m.height as i32);
        }
        rasters.push((m, bitmap));
    }
    let width = (max_xmax - min_xmin + PAD * 2).clamp(6, 10) as u32;
    let height = (max_ymax - min_ymin + PAD * 2).clamp(12, 18) as u32;
    let place = Place {
        min_xmin,
        max_ymax,
        pad: PAD,
        width,
        height,
    };

    let mut out = String::new();
    out.push_str("pub const WIDTH: u32 = ");
    out.push_str(&width.to_string());
    out.push_str(";\npub const HEIGHT: u32 = ");
    out.push_str(&height.to_string());
    out.push_str(";\npub static GLYPHS: [[u8; ");
    out.push_str(&(width * height).to_string());
    out.push_str("]; 95] = [\n");

    for (metrics, bitmap) in &rasters {
        let cell = pack_cell(metrics, bitmap, place);
        out.push_str("    [");
        for (i, b) in cell.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&b.to_string());
        }
        out.push_str("],\n");
    }
    out.push_str("];\n");

    let dest = PathBuf::from(env::var("OUT_DIR").unwrap()).join("font_atlas.rs");
    fs::write(&dest, out).expect("write font atlas");
}

#[derive(Clone, Copy)]
struct Place {
    min_xmin: i32,
    max_ymax: i32,
    pad: i32,
    width: u32,
    height: u32,
}

fn pack_cell(metrics: &fontdue::Metrics, bitmap: &[u8], p: Place) -> Vec<u8> {
    let mut cell = vec![0u8; (p.width * p.height) as usize];
    for row in 0..metrics.height {
        for col in 0..metrics.width {
            let dx = col as i32 + metrics.xmin - p.min_xmin + p.pad;
            // fontdue ymin is the bitmap bottom vs baseline (y-up). Row 0 is the top.
            let dy = row as i32 + p.pad + p.max_ymax - metrics.ymin - metrics.height as i32;
            if dx < 0 || dy < 0 || dx >= p.width as i32 || dy >= p.height as i32 {
                continue;
            }
            let src = row * metrics.width + col;
            let cov = bitmap.get(src).copied().unwrap_or(0);
            let dst = (dy as u32 * p.width + dx as u32) as usize;
            if cov > cell[dst] {
                cell[dst] = cov;
            }
        }
    }
    cell
}
