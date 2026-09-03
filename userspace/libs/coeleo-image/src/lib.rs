//! JPEG/PNG decode for wallpaper and the file manager. `no_std` + alloc.
//!
//! PNG: `zune-png`. JPEG: vendored `stb_image` in the kernel (`-mno-sse`);
//! `zune-jpeg` hangs on `x86_64-unknown-none` even for a 160×100 baseline.

#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;
use zune_png::PngDecoder;

pub const MAX_BYTES: usize = 1536 * 1024;
pub const MAX_SIDE: u32 = 2048;

unsafe extern "C" {
    fn stbi_load_from_memory(
        buffer: *const u8,
        len: i32,
        x: *mut i32,
        y: *mut i32,
        channels: *mut i32,
        desired: i32,
    ) -> *mut u8;
    fn stbi_image_free(retval: *mut u8);
}

pub fn decode_rgba(bytes: &[u8]) -> Result<(u32, u32, Vec<u32>), ()> {
    if bytes.len() > MAX_BYTES || bytes.len() < 8 {
        return Err(());
    }
    if bytes[0] == 0xFF && bytes[1] == 0xD8 {
        decode_jpeg(bytes)
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        decode_png(bytes)
    } else {
        Err(())
    }
}

fn opts() -> DecoderOptions {
    DecoderOptions::default().set_use_unsafe(false)
}

fn decode_jpeg(bytes: &[u8]) -> Result<(u32, u32, Vec<u32>), ()> {
    if bytes.len() > i32::MAX as usize {
        return Err(());
    }
    let mut w = 0i32;
    let mut h = 0i32;
    let mut n = 0i32;
    let ptr = unsafe {
        stbi_load_from_memory(
            bytes.as_ptr(),
            bytes.len() as i32,
            &mut w,
            &mut h,
            &mut n,
            4,
        )
    };
    if ptr.is_null() || w <= 0 || h <= 0 {
        return Err(());
    }
    let (uw, uh) = (w as u32, h as u32);
    let len = (w as usize).saturating_mul(h as usize).saturating_mul(4);
    let raw = unsafe { core::slice::from_raw_parts(ptr, len) };
    let out = pack_rgba(raw, uw, uh, 4);
    unsafe { stbi_image_free(ptr) };
    out
}

fn decode_png(bytes: &[u8]) -> Result<(u32, u32, Vec<u32>), ()> {
    let mut dec = PngDecoder::new_with_options(bytes, opts());
    let raw = dec.decode_raw().map_err(|_| ())?;
    let (w, h) = dec.get_dimensions().ok_or(())?;
    if matches!(dec.get_depth(), Some(zune_core::bit_depth::BitDepth::Sixteen)) {
        return Err(());
    }
    let n = match dec.get_colorspace() {
        Some(ColorSpace::RGBA) => 4,
        Some(ColorSpace::RGB) => 3,
        Some(ColorSpace::Luma) => 1,
        Some(ColorSpace::LumaA) => 2,
        _ => return Err(()),
    };
    pack_rgba(&raw, w as u32, h as u32, n)
}

fn pack_rgba(raw: &[u8], w: u32, h: u32, stride: usize) -> Result<(u32, u32, Vec<u32>), ()> {
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
        return Err(());
    }
    let n = (w as usize).saturating_mul(h as usize);
    if raw.len() < n.saturating_mul(stride) {
        return Err(());
    }
    let mut out = Vec::new();
    out.try_reserve(n).map_err(|_| ())?;
    for i in 0..n {
        let o = i * stride;
        let (r, g, b, a) = match stride {
            4 => (raw[o], raw[o + 1], raw[o + 2], raw[o + 3]),
            3 => (raw[o], raw[o + 1], raw[o + 2], 255),
            2 => (raw[o], raw[o], raw[o], raw[o + 1]),
            _ => (raw[o], raw[o], raw[o], 255),
        };
        out.push(u32::from(b) | (u32::from(g) << 8) | (u32::from(r) << 16) | (u32::from(a) << 24));
    }
    Ok((w, h, out))
}

pub fn scale_nn(src: &[u32], sw: u32, sh: u32, dw: u32, dh: u32) -> Result<Vec<u32>, ()> {
    if dw == 0 || dh == 0 || sw == 0 || sh == 0 {
        return Err(());
    }
    let n = (dw as usize).saturating_mul(dh as usize);
    let mut out = Vec::new();
    out.try_reserve(n).map_err(|_| ())?;
    out.resize(n, 0);
    for y in 0..dh {
        let sy = y.saturating_mul(sh) / dh;
        for x in 0..dw {
            let sx = x.saturating_mul(sw) / dw;
            out[(y * dw + x) as usize] = src[(sy * sw + sx) as usize];
        }
    }
    Ok(out)
}

fn lerp_u8(a: u32, b: u32, t: u32) -> u32 {
    (a * (256 - t) + b * t) / 256
}

fn lerp_px(a: u32, b: u32, t: u32) -> u32 {
    let bb = lerp_u8(a & 0xff, b & 0xff, t);
    let gb = lerp_u8((a >> 8) & 0xff, (b >> 8) & 0xff, t);
    let rb = lerp_u8((a >> 16) & 0xff, (b >> 16) & 0xff, t);
    let ab = lerp_u8((a >> 24) & 0xff, (b >> 24) & 0xff, t);
    bb | (gb << 8) | (rb << 16) | (ab << 24)
}

fn sample(src: &[u32], sw: u32, sh: u32, x: u32, y: u32) -> u32 {
    src[(y.min(sh.saturating_sub(1)) * sw + x.min(sw.saturating_sub(1))) as usize]
}

/// Bilinear upscale. Used when the destination is larger than the source.
pub fn scale_lerp(src: &[u32], sw: u32, sh: u32, dw: u32, dh: u32) -> Result<Vec<u32>, ()> {
    if dw == 0 || dh == 0 || sw == 0 || sh == 0 {
        return Err(());
    }
    let n = (dw as usize).saturating_mul(dh as usize);
    let mut out = Vec::new();
    out.try_reserve(n).map_err(|_| ())?;
    out.resize(n, 0);
    let xden = dw.max(1) as u64;
    let yden = dh.max(1) as u64;
    for y in 0..dh {
        let fy = ((y as u64) * (sh.saturating_sub(1) as u64) * 256) / yden;
        let y0 = (fy / 256) as u32;
        let y1 = y0.saturating_add(1).min(sh.saturating_sub(1));
        let ty = (fy % 256) as u32;
        for x in 0..dw {
            let fx = ((x as u64) * (sw.saturating_sub(1) as u64) * 256) / xden;
            let x0 = (fx / 256) as u32;
            let x1 = x0.saturating_add(1).min(sw.saturating_sub(1));
            let tx = (fx % 256) as u32;
            let p00 = sample(src, sw, sh, x0, y0);
            let p10 = sample(src, sw, sh, x1, y0);
            let p01 = sample(src, sw, sh, x0, y1);
            let p11 = sample(src, sw, sh, x1, y1);
            let top = lerp_px(p00, p10, tx);
            let bot = lerp_px(p01, p11, tx);
            out[(y * dw + x) as usize] = lerp_px(top, bot, ty);
        }
    }
    Ok(out)
}

/// Area-average downsample (box). Upscale uses bilinear (`scale_lerp`).
pub fn scale_box(src: &[u32], sw: u32, sh: u32, dw: u32, dh: u32) -> Result<Vec<u32>, ()> {
    if dw >= sw && dh >= sh {
        return scale_lerp(src, sw, sh, dw, dh);
    }
    if dw == 0 || dh == 0 || sw == 0 || sh == 0 {
        return Err(());
    }
    let n = (dw as usize).saturating_mul(dh as usize);
    let mut out = Vec::new();
    out.try_reserve(n).map_err(|_| ())?;
    out.resize(n, 0);
    for y in 0..dh {
        let y0 = (y as u64).saturating_mul(sh as u64) / dh as u64;
        let y1 = (y as u64 + 1)
            .saturating_mul(sh as u64)
            .saturating_div(dh as u64)
            .max(y0 + 1)
            .min(sh as u64);
        for x in 0..dw {
            let x0 = (x as u64).saturating_mul(sw as u64) / dw as u64;
            let x1 = (x as u64 + 1)
                .saturating_mul(sw as u64)
                .saturating_div(dw as u64)
                .max(x0 + 1)
                .min(sw as u64);
            let mut bb = 0u64;
            let mut gb = 0u64;
            let mut rb = 0u64;
            let mut ab = 0u64;
            let mut cnt = 0u64;
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let p = src[(sy * sw as u64 + sx) as usize];
                    bb += u64::from(p & 0xff);
                    gb += u64::from((p >> 8) & 0xff);
                    rb += u64::from((p >> 16) & 0xff);
                    ab += u64::from((p >> 24) & 0xff);
                    cnt += 1;
                }
            }
            if cnt == 0 {
                continue;
            }
            out[(y * dw + x) as usize] = ((bb / cnt) as u32)
                | (((gb / cnt) as u32) << 8)
                | (((rb / cnt) as u32) << 16)
                | (((ab / cnt) as u32) << 24);
        }
    }
    Ok(out)
}
