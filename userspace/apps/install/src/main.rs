#![no_std]
#![no_main]
#![allow(static_mut_refs)]

use coeleo_theme::{ACCENT, BUTTON_H, DIM, HIGHLIGHT, PAD, SURFACE, TEXT};
use libcoeleo::{
    DISK_FLAG_LIVE, DISK_FLAG_SMALL, DISK_KIND_AHCI, DISK_KIND_USB, DISK_KIND_VIRTIO, ERR, disks,
    install, reboot, sync, write,
};
use libcoeleoui::{KEY_DOWN, KEY_ENTER, KEY_ESC, KEY_LEFT, KEY_RIGHT, KEY_UP, ROW};

const W: u32 = 256;
const H: u32 = 240;
const NPX: usize = 256 * 240;
const FOOT_Y: u32 = 204;
const BTN_H: u32 = BUTTON_H;
const BTN_W: u32 = 116;
const CARD_X: u32 = 12;
const CARD_Y: u32 = 48;
const CARD_W: u32 = 232;
const CARD_H: u32 = 148;
const ROW_H: u32 = ROW;
const MAX_DISKS: usize = 8;

static mut PIX: [u32; NPX] = [0; NPX];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Welcome,
    Disks,
    Erase,
    Progress,
    Done,
    Failed,
}

#[derive(Clone, Copy)]
struct Row {
    index: u32,
    kind: u32,
    flags: u32,
    sectors: u64,
}

struct Wizard {
    page: Page,
    rows: [Row; MAX_DISKS],
    n: usize,
    sel: usize,
    erase_ok: bool,
    apply: bool,
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let id = libcoeleo::win_create(W, H, unsafe { &PIX });
    if id == ERR {
        libcoeleo::exit(1);
    }
    let _ = libcoeleo::win_damage(id, 0, 0, W, H);

    let mut ui = libcoeleoui::Ui::new();
    let mut raw = [0u8; 128];
    let mut wz = Wizard {
        page: Page::Welcome,
        rows: [Row {
            index: 0,
            kind: 0,
            flags: 0,
            sectors: 0,
        }; MAX_DISKS],
        n: 0,
        sel: 0,
        erase_ok: false,
        apply: false,
    };

    loop {
        let n = libcoeleo::poll_input(&mut raw);
        if n != ERR && n > 0 {
            let n = (n as usize).min(raw.len());
            ui.feed(&raw[..n]);
        }

        if wz.page == Page::Progress && !wz.apply {
            ui.begin(unsafe { &mut PIX }, W, H);
            chrome(&mut ui, 3);
            paint_progress(&mut ui);
            if ui.end() {
                let _ = libcoeleo::win_damage(id, 0, 0, W, H);
            }
            wz.apply = true;
            let idx = wz.rows[wz.sel].index as u64;
            wz.page = if install(idx) == 0 {
                Page::Done
            } else {
                Page::Failed
            };
            continue;
        }

        ui.begin(unsafe { &mut PIX }, W, H);
        match wz.page {
            Page::Welcome => page_welcome(&mut ui, &mut wz),
            Page::Disks => page_disks(&mut ui, &mut wz),
            Page::Erase => page_erase(&mut ui, &mut wz),
            Page::Progress => {
                chrome(&mut ui, 3);
                paint_progress(&mut ui);
            }
            Page::Done => page_done(&mut ui),
            Page::Failed => page_failed(&mut ui),
        }
        if ui.end() {
            let _ = libcoeleo::win_damage(id, 0, 0, W, H);
        }
    }
}

fn page_welcome(ui: &mut libcoeleoui::Ui, wz: &mut Wizard) {
    chrome(ui, 0);
    ui.label(PAD, 48, "This installs Coeleo onto");
    ui.label(PAD, 64, "the disk you pick. That");
    ui.label(PAD, 80, "disk is erased.");
    let go = ui.button(132, FOOT_Y, BTN_W, BTN_H, "Continue") || ui.key() == Some(KEY_ENTER);
    if go {
        load_disks(wz);
        wz.page = Page::Disks;
    }
}

fn page_disks(ui: &mut libcoeleoui::Ui, wz: &mut Wizard) {
    chrome(ui, 1);
    ui.label_color(PAD, 44, "Choose a disk. The disk", DIM);
    ui.label_color(PAD, 58, "Coeleo is running from", DIM);
    ui.label_color(PAD, 72, "cannot be used.", DIM);

    let list_y = 90u32;
    let mut line = [0u8; 40];
    for i in 0..wz.n {
        let y = list_y + i as u32 * ROW_H;
        let s = fmt_row(&mut line, &wz.rows[i]);
        if ui.list_row(PAD, y, W - PAD * 2, s, i == wz.sel) {
            wz.sel = i;
        }
    }
    match ui.key() {
        Some(KEY_UP) if wz.sel > 0 => wz.sel -= 1,
        Some(KEY_DOWN) if wz.sel + 1 < wz.n => wz.sel += 1,
        _ => {}
    }

    ui.label_color(PAD, 184, status(wz), DIM);

    let back = ui.button(PAD, FOOT_Y, BTN_W, BTN_H, "Back");
    let can = usable(wz);
    let next = ui.button_en(132, FOOT_Y, BTN_W, BTN_H, "Next", SURFACE, TEXT, can)
        || (ui.key() == Some(KEY_ENTER) && can);
    if back {
        wz.page = Page::Welcome;
    } else if next && can {
        wz.erase_ok = false;
        wz.page = Page::Erase;
    }
}

fn page_erase(ui: &mut libcoeleoui::Ui, wz: &mut Wizard) {
    chrome(ui, 2);
    ui.toolbar(CARD_X, CARD_Y, CARD_W, CARD_H);
    let mut title = [0u8; 48];
    ui.label(PAD + 4, 56, erase_title(&mut title, wz));
    ui.label(PAD + 4, 80, "All data on this disk will");
    ui.label(PAD + 4, 96, "be destroyed. This cannot");
    ui.label(PAD + 4, 112, "be undone.");

    match ui.key() {
        Some(KEY_LEFT) | Some(KEY_RIGHT) => wz.erase_ok = !wz.erase_ok,
        Some(KEY_ESC) => {
            wz.page = Page::Disks;
            return;
        }
        Some(KEY_ENTER) => {
            if wz.erase_ok {
                commit(wz);
            } else {
                wz.page = Page::Disks;
            }
            return;
        }
        _ => {}
    }

    let cancel_fill = if wz.erase_ok { SURFACE } else { HIGHLIGHT };
    let cancel = ui.button_fill(PAD, FOOT_Y, BTN_W, BTN_H, "Cancel", cancel_fill);
    let wipe = ui.button_danger(132, FOOT_Y, BTN_W, BTN_H, "Erase disk");
    let on_footer = ui.hovering(0, FOOT_Y, W, BTN_H);
    let scrim = ui.down_outside(CARD_X, CARD_Y, CARD_W, CARD_H) && !on_footer;
    if cancel || scrim {
        wz.page = Page::Disks;
    } else if wipe && wz.erase_ok {
        commit(wz);
    }
}

fn page_done(ui: &mut libcoeleoui::Ui) {
    chrome(ui, 4);
    ui.label(PAD, 56, "Coeleo is ready on this");
    ui.label(PAD, 72, "disk.");
    if ui.button(PAD, FOOT_Y, BTN_W, BTN_H, "Close") {
        libcoeleo::exit(0);
    }
    if ui.button(132, FOOT_Y, BTN_W, BTN_H, "Reboot now") || ui.key() == Some(KEY_ENTER) {
        let _ = sync();
        let _ = reboot();
        let _ = write(1, b"reboot: failed\n");
    }
}

fn page_failed(ui: &mut libcoeleoui::Ui) {
    chrome(ui, 3);
    ui.label(PAD, 56, "Install failed.");
    if ui.button(PAD, FOOT_Y, BTN_W, BTN_H, "Close") || ui.key() == Some(KEY_ENTER) {
        libcoeleo::exit(1);
    }
}

fn commit(wz: &mut Wizard) {
    let _ = write(1, b"install: confirm\n");
    let idx = wz.rows[wz.sel].index as u64;
    if install(idx) != 1 {
        wz.page = Page::Failed;
        return;
    }
    wz.apply = false;
    wz.page = Page::Progress;
}

fn paint_progress(ui: &mut libcoeleoui::Ui) {
    ui.label(PAD, 48, "Keep the machine on.");
    ui.label_color(PAD, 80, "> Partition", ACCENT);
    ui.label_color(PAD, 98, "  Format", DIM);
    ui.label_color(PAD, 116, "  Copy", DIM);
    ui.label_color(PAD, 134, "  Bootloader", DIM);
}

fn chrome(ui: &mut libcoeleoui::Ui, step: usize) {
    ui.label(PAD, 8, "Install Coeleo");
    let mut x = PAD;
    for i in 0..5 {
        let c = if i < step {
            HIGHLIGHT
        } else if i == step {
            ACCENT
        } else {
            DIM
        };
        ui.disc(x, 28, 8, c);
        x += 14;
    }
}

fn load_disks(wz: &mut Wizard) {
    let mut buf = [0u8; 256];
    let r = disks(&mut buf);
    wz.n = 0;
    wz.sel = 0;
    if r == ERR || r < 8 {
        return;
    }
    let count = le_u32(&buf, 0).min(MAX_DISKS as u32) as usize;
    let mut first_ok = None;
    for i in 0..count {
        let o = 8 + i * 24;
        if o + 24 > buf.len() {
            break;
        }
        let row = Row {
            index: le_u32(&buf, o),
            kind: le_u32(&buf, o + 4),
            flags: le_u32(&buf, o + 8),
            sectors: le_u64(&buf, o + 16),
        };
        if first_ok.is_none() && row.flags & (DISK_FLAG_LIVE | DISK_FLAG_SMALL) == 0 {
            first_ok = Some(wz.n);
        }
        wz.rows[wz.n] = row;
        wz.n += 1;
    }
    if let Some(i) = first_ok {
        wz.sel = i;
    }
}

fn usable(wz: &Wizard) -> bool {
    wz.n > 0 && wz.rows[wz.sel].flags & (DISK_FLAG_LIVE | DISK_FLAG_SMALL) == 0
}

fn status(wz: &Wizard) -> &'static str {
    if wz.n == 0 {
        return "No disks.";
    }
    let f = wz.rows[wz.sel].flags;
    if f & DISK_FLAG_LIVE != 0 {
        "This disk is running Coeleo."
    } else if f & DISK_FLAG_SMALL != 0 {
        "This disk cannot be used."
    } else {
        "This disk will be erased."
    }
}

fn kind_name(kind: u32) -> &'static str {
    match kind {
        DISK_KIND_AHCI => "SATA",
        DISK_KIND_VIRTIO => "virtio",
        DISK_KIND_USB => "USB",
        3 => "NVMe",
        _ => "disk",
    }
}

fn fmt_row<'a>(buf: &'a mut [u8; 40], row: &Row) -> &'a str {
    let mut i = 0usize;
    push_u32(buf, &mut i, row.index);
    push_str(buf, &mut i, "  ");
    push_str(buf, &mut i, kind_name(row.kind));
    push_str(buf, &mut i, "  ");
    let mib = row.sectors / 2048;
    push_u32(buf, &mut i, mib as u32);
    push_str(buf, &mut i, " MiB");
    if row.flags & DISK_FLAG_LIVE != 0 {
        push_str(buf, &mut i, "  LIVE");
    }
    core::str::from_utf8(&buf[..i]).unwrap_or("")
}

fn erase_title<'a>(buf: &'a mut [u8; 48], wz: &Wizard) -> &'a str {
    let row = &wz.rows[wz.sel];
    let mut i = 0usize;
    push_str(buf, &mut i, "Erase ");
    push_str(buf, &mut i, kind_name(row.kind));
    push_str(buf, &mut i, " disk ");
    push_u32(buf, &mut i, row.index);
    push_str(buf, &mut i, " (");
    push_u32(buf, &mut i, (row.sectors / 2048) as u32);
    push_str(buf, &mut i, " MiB)?");
    core::str::from_utf8(&buf[..i]).unwrap_or("")
}

fn le_u32(buf: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]])
}

fn le_u64(buf: &[u8], o: usize) -> u64 {
    u64::from_le_bytes([
        buf[o],
        buf[o + 1],
        buf[o + 2],
        buf[o + 3],
        buf[o + 4],
        buf[o + 5],
        buf[o + 6],
        buf[o + 7],
    ])
}

fn push_str(buf: &mut [u8], i: &mut usize, s: &str) {
    let b = s.as_bytes();
    let n = b.len().min(buf.len().saturating_sub(*i));
    buf[*i..*i + n].copy_from_slice(&b[..n]);
    *i += n;
}

fn push_u32(buf: &mut [u8], i: &mut usize, mut n: u32) {
    if n == 0 {
        push_str(buf, i, "0");
        return;
    }
    let mut tmp = [0u8; 10];
    let mut k = 0usize;
    while n > 0 && k < tmp.len() {
        tmp[k] = b'0' + (n % 10) as u8;
        n /= 10;
        k += 1;
    }
    while k > 0 {
        k -= 1;
        if *i < buf.len() {
            buf[*i] = tmp[k];
            *i += 1;
        }
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
