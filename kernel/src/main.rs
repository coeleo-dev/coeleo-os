#![feature(abi_x86_interrupt)]
#![no_std]
#![no_main]

extern crate alloc;

use core::fmt::Write;

use limine::BaseRevision;
use limine::request::{
    ExecutableAddressRequest, FramebufferRequest, HhdmRequest, MemoryMapRequest, MpRequest,
    RequestsEndMarker, RequestsStartMarker, RsdpRequest,
};

mod boot;
mod bus;
mod fs;
mod input;
mod mem;
mod net;
mod task;
mod ui;

#[allow(unused_imports)]
pub(crate) use boot::{
    acpi, console, gdt, init, interrupts, irqlock, kdebug, lapic, rtc, serial, shell, smp,
};
pub(crate) use bus::{pci, xhci};
pub(crate) use fs::{ahci, blk, fat_disk, gpt, install, part, usb_msc};
pub(crate) use input::{kbd, mouse, ps2, uhci, usb_hid};
pub(crate) use mem::{heap, pmm, vmm};
pub(crate) use net::{e1000e, http, virtio_hal, virtio_net, virtio_pci};
pub(crate) use task::{elfload, fd, percpu, pipe, process, sched, syscall};
#[allow(unused_imports)]
pub(crate) use ui::font8x16;
pub(crate) use ui::{clock, comp, desk, deskset, fbterm, fm, krunner, panel, splash, win};

/// Sets the base revision to the latest revision supported by the crate.
/// See specification for further info.
/// Be sure to mark all limine requests with #[used], otherwise they may be removed by the compiler.
#[used]
#[unsafe(link_section = ".requests")]
static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
#[unsafe(link_section = ".requests")]
static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
static MEMORY_MAP_REQUEST: MemoryMapRequest = MemoryMapRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
static EXECUTABLE_ADDRESS_REQUEST: ExecutableAddressRequest = ExecutableAddressRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
static RSDP_REQUEST: RsdpRequest = RsdpRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
static MP_REQUEST: MpRequest = MpRequest::new();

#[used]
#[unsafe(link_section = ".requests_start_marker")]
static _START_MARKER: RequestsStartMarker = RequestsStartMarker::new();
#[used]
#[unsafe(link_section = ".requests_end_marker")]
static _END_MARKER: RequestsEndMarker = RequestsEndMarker::new();

#[unsafe(no_mangle)]
unsafe extern "C" fn kmain() -> ! {
    serial::init();

    // All limine requests must also be referenced in a called function, otherwise they may be
    // removed by the linker.
    assert!(BASE_REVISION.is_supported());

    if let Some(framebuffer_response) = FRAMEBUFFER_REQUEST.get_response() {
        if let Some(framebuffer) = framebuffer_response.framebuffers().next() {
            let _ = fbterm::init(&framebuffer);
        }
    }

    console::boot_banner();

    if !fbterm::is_ready() {
        serial::write_str("\nno framebuffer\n");
    }

    let hhdm = HHDM_REQUEST.get_response().expect("HHDM");
    let memmap = MEMORY_MAP_REQUEST.get_response().expect("memmap");
    let _ = EXECUTABLE_ADDRESS_REQUEST
        .get_response()
        .expect("executable address");

    vmm::init(hhdm.offset());
    pmm::init(memmap.entries());
    heap::init();
    splash::start();
    gdt::init_bsp();
    interrupts::init_bsp();
    acpi::init(RSDP_REQUEST.get_response().map(|r| r.address()));
    syscall::init();
    sched::init();
    sched::init_template();
    lapic::init_bsp();
    if let Some(mp) = MP_REQUEST.get_response() {
        let cpus = mp.cpus().len();
        let bsp = mp.bsp_lapic_id();
        let started = smp::start_aps(mp);
        serial_println!("smp: {cpus} cpus, bsp apic {bsp}, {started} aps");
    } else {
        serial::write_str("smp: no mp response\n");
    }
    blk::init();
    splash::tick();
    net::init();
    splash::tick();
    fs::init();
    ps2::init();
    splash::tick();
    uhci::init();
    usb_hid::init();
    if uhci::present() || usb_hid::mouse_present() {
        serial::write_str("mouse: usb\n");
    } else if !ps2::mouse_present() {
        serial::write_str("mouse: none\n");
    }
    if !fbterm::attach_vt_default() {
        serial::write_str("vt: attach failed\n");
    }
    console::boot_hero();
    comp::init();
    interrupts::unmask_ps2();
    interrupts::enable();
    loop {
        if !init::try_sh() {
            break;
        }
    }
    console::write("\x1b[1;96mcoeleo>\x1b[0m");

    loop {
        while let Some(b) = kbd::pop_byte() {
            shell::handle_byte(b);
        }
        comp::poll();
        net::poll();
        clock::paint_if_second_elapsed();
        interrupts::wait();
    }
}

struct PanicBuf {
    buf: [u8; 512],
    pos: usize,
}

impl PanicBuf {
    const fn new() -> Self {
        Self {
            buf: [0; 512],
            pos: 0,
        }
    }

    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.pos]).unwrap_or("panic: (unprintable)")
    }
}

impl Write for PanicBuf {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let rest = &mut self.buf[self.pos..];
        let n = s.len().min(rest.len());
        rest[..n].copy_from_slice(&s.as_bytes()[..n]);
        self.pos += n;
        Ok(())
    }
}

#[panic_handler]
fn rust_panic(info: &core::panic::PanicInfo) -> ! {
    let mut buf = PanicBuf::new();
    let _ = write!(buf, "panic: {info}");
    console::write(buf.as_str());
    console::write("\n");
    hcf();
}

pub(crate) fn hcf() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}
