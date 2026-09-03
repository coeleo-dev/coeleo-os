#![no_std]
#![no_main]
#![allow(static_mut_refs)]

const W: u32 = 192;
const H: u32 = 80;
const NPX: usize = 192 * 80;

static mut PIX: [u32; NPX] = [0; NPX];

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let id = libcoeleo::win_create(W, H, unsafe { &PIX });
    if id == libcoeleo::ERR {
        libcoeleo::exit(1);
    }
    let _ = libcoeleo::win_damage(id, 0, 0, W, H);

    let mut ui = libcoeleoui::Ui::new();
    let mut field = [0u8; 32];
    let mut ok = false;
    let mut raw = [0u8; 128];
    loop {
        let n = libcoeleo::poll_input(&mut raw);
        if n != libcoeleo::ERR && n > 0 {
            let n = (n as usize).min(raw.len());
            ui.feed(&raw[..n]);
        }
        ui.begin(unsafe { &mut PIX }, W, H);
        ui.search_field(8, 28, 160, &mut field);
        if ui.button(8, 48, 96, 32, "ok") {
            ok = true;
            let _ = libcoeleo::write(1, b"ui: clicked\n");
        }
        if ui.hovering(8, 48, 96, 32) {
            ui.tooltip(110, 52, "ok");
        }
        ui.label(8, 8, if ok { "ok" } else { "ready" });
        if ui.end() {
            let _ = libcoeleo::win_damage(id, 0, 0, W, H);
        }
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
