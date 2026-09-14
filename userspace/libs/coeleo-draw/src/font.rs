//! Grayscale GUI atlas (Ubuntu Regular, 11 pt). Generated at build.
//! The VT Flanterm bitmap is unrelated.

include!(concat!(env!("OUT_DIR"), "/font_atlas.rs"));

pub fn idx(c: u8) -> usize {
    if (32..127).contains(&c) {
        (c - 32) as usize
    } else {
        0
    }
}

pub fn advance(c: u8) -> u32 {
    ADVANCE[idx(c)] as u32
}

pub fn coverage(c: u8, gx: u32, gy: u32) -> u8 {
    if gx >= WIDTH || gy >= HEIGHT {
        return 0;
    }
    GLYPHS[idx(c)][(gy * WIDTH + gx) as usize]
}
