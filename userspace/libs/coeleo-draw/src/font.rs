//! Grayscale GUI atlas (~11 px). Generated at build from IBM Plex Mono.
//! The VT Flanterm bitmap is unrelated.

include!(concat!(env!("OUT_DIR"), "/font_atlas.rs"));

pub fn coverage(c: u8, gx: u32, gy: u32) -> u8 {
    if gx >= WIDTH || gy >= HEIGHT {
        return 0;
    }
    let i = if (32..127).contains(&c) {
        (c - 32) as usize
    } else {
        0
    };
    GLYPHS[i][(gy * WIDTH + gx) as usize]
}
