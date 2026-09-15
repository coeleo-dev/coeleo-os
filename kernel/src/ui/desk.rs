//! Boot-time wallpaper, menu icon, and `/desk.cfg`.

use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use crate::fs::{self, FsError};
use crate::panel;
use crate::serial;

const WALL_JPG: &[u8] = include_bytes!("../../../docs/image/wallpaper.jpg");
const ICON_PNG: &[u8] = include_bytes!("../../../docs/image/Union.png");
const CFG_PATH: &str = "/desk.cfg";
const MENU_ICON: u32 = 24;

pub struct Wallpaper {
    pub w: u32,
    pub h: u32,
    pub pix: Vec<u32>,
}

#[derive(Clone)]
pub enum WallSrc {
    Default,
    Path(String),
}

static WALL_SRC: Mutex<WallSrc> = Mutex::new(WallSrc::Default);

pub fn wall_src() -> WallSrc {
    match &*WALL_SRC.lock() {
        WallSrc::Default => WallSrc::Default,
        WallSrc::Path(p) => WallSrc::Path(p.clone()),
    }
}

pub fn set_wall_src(src: WallSrc) {
    *WALL_SRC.lock() = src;
}

pub fn load_current(fb_w: u32, h_work: u32) -> Option<Wallpaper> {
    match wall_src() {
        WallSrc::Default => load_wallpaper(fb_w, h_work),
        WallSrc::Path(p) => {
            let bytes = fs::read_file(&p).ok()?;
            wallpaper_from_bytes(&bytes, fb_w, h_work)
        }
    }
}

pub struct DeskCfg {
    pub mode: panel::Mode,
    pub wall: WallSrc,
}

pub fn load_wallpaper(fb_w: u32, h_work: u32) -> Option<Wallpaper> {
    wallpaper_from_bytes(WALL_JPG, fb_w, h_work)
}

pub fn wallpaper_from_bytes(bytes: &[u8], fb_w: u32, h_work: u32) -> Option<Wallpaper> {
    let (w, h, pix) = coeleo_image::decode_rgba(bytes).ok()?;
    if fb_w == 0 || h_work == 0 {
        return None;
    }
    match coeleo_image::scale_box(&pix, w, h, fb_w, h_work) {
        Ok(pix) => Some(Wallpaper {
            w: fb_w,
            h: h_work,
            pix,
        }),
        Err(()) => None,
    }
}

pub fn load_menu_icon() -> Option<(u32, u32, Vec<u32>)> {
    let (w, h, pix) = coeleo_image::decode_rgba(ICON_PNG).ok()?;
    let pix = coeleo_image::scale_box(&pix, w, h, MENU_ICON, MENU_ICON).ok()?;
    Some((MENU_ICON, MENU_ICON, pix))
}

pub fn load_cfg() -> Option<DeskCfg> {
    let bytes = fs::read_file(CFG_PATH).ok()?;
    parse_cfg(&bytes)
}

fn parse_cfg(bytes: &[u8]) -> Option<DeskCfg> {
    let text = core::str::from_utf8(bytes).ok()?;
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let mode = match lines.next()?.trim() {
        "full" => panel::Mode::Full,
        "float" => panel::Mode::Float,
        _ => return None,
    };
    let wall = match lines.next()?.trim() {
        "default" => WallSrc::Default,
        p if valid_wall_path(p) => WallSrc::Path(String::from(p)),
        _ => return None,
    };
    Some(DeskCfg { mode, wall })
}

fn valid_wall_path(p: &str) -> bool {
    p.starts_with('/') && !p.contains("..") && p.len() < 96
}

pub fn save_cfg(cfg: &DeskCfg) {
    let mode = match cfg.mode {
        panel::Mode::Float => "float",
        panel::Mode::Full => "full",
    };
    let wall = match &cfg.wall {
        WallSrc::Default => "default",
        WallSrc::Path(p) => p.as_str(),
    };
    let mut buf = String::new();
    buf.push_str(mode);
    buf.push('\n');
    buf.push_str(wall);
    buf.push('\n');
    match fs::write_file(CFG_PATH, buf.as_bytes()) {
        Ok(()) => {
            // if fs::sync().is_err() {
            //     serial::write_str("desk: cfg sync fail\n");
            // }
        }
        Err(FsError::NoFs) => serial::write_str("desk: cfg no fs\n"),
        Err(_) => serial::write_str("desk: cfg write fail\n"),
    }
}
