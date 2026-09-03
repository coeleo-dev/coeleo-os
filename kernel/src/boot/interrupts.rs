//! IDT, PIC 8259 (keyboard), and LAPIC timer vector.

use pic8259::ChainedPics;
use spin::{Mutex, Once};
use x86_64::PrivilegeLevel;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

use crate::lapic;
use crate::ps2;
use x86_64::VirtAddr;

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;
pub const KEYBOARD_VECTOR: u8 = PIC_1_OFFSET + 1;
pub const MOUSE_VECTOR: u8 = PIC_1_OFFSET + 12;
const SPURIOUS_MASTER: u8 = PIC_1_OFFSET + 7;

static PICS: Mutex<ChainedPics> =
    Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

static IDT: Once<InterruptDescriptorTable> = Once::new();

pub fn init() {
    let idt = IDT.call_once(build_idt);
    idt.load();

    let mut pics = PICS.lock();
    unsafe {
        pics.initialize();
        pics.write_masks(0xFF, 0xFF);
    }
}

pub fn unmask_ps2() {
    // Bit set in the PIC mask means ignored. IRQ0 (PIT) stays masked.
    // IRQ1 keyboard, IRQ2 cascade, IRQ12 mouse.
    unsafe {
        PICS.lock().write_masks(0b1111_1001, 0b1110_1111);
    }
}

pub fn enable() {
    x86_64::instructions::interrupts::enable();
}

pub fn wait() {
    x86_64::instructions::interrupts::disable();
    if ps2::is_empty()
        && crate::kbd::bytes_empty()
        && crate::uhci::idle()
        && crate::usb_hid::idle()
        && crate::comp::keys_empty()
    {
        x86_64::instructions::interrupts::enable_and_hlt();
    } else {
        x86_64::instructions::interrupts::enable();
    }
}

fn eoi(vector: u8) {
    unsafe {
        PICS.lock().notify_end_of_interrupt(vector);
    }
}

fn build_idt() -> InterruptDescriptorTable {
    let mut idt = InterruptDescriptorTable::new();
    idt.divide_error.set_handler_fn(divide_error);
    idt.debug.set_handler_fn(debug);
    unsafe {
        idt.non_maskable_interrupt
            .set_handler_fn(non_maskable_interrupt)
            .set_stack_index(crate::gdt::DOUBLE_FAULT_IST_INDEX);
    }
    idt.breakpoint.set_handler_fn(breakpoint);
    idt.overflow.set_handler_fn(overflow);
    idt.bound_range_exceeded
        .set_handler_fn(bound_range_exceeded);
    idt.invalid_opcode.set_handler_fn(invalid_opcode);
    idt.device_not_available
        .set_handler_fn(device_not_available);
    unsafe {
        idt.double_fault
            .set_handler_fn(double_fault)
            .set_stack_index(crate::gdt::DOUBLE_FAULT_IST_INDEX);
    }
    idt.invalid_tss.set_handler_fn(invalid_tss);
    idt.segment_not_present.set_handler_fn(segment_not_present);
    idt.stack_segment_fault.set_handler_fn(stack_segment_fault);
    idt.general_protection_fault
        .set_handler_fn(general_protection_fault);
    idt.page_fault.set_handler_fn(page_fault);
    idt.x87_floating_point.set_handler_fn(x87_floating_point);
    idt.alignment_check.set_handler_fn(alignment_check);
    idt.machine_check.set_handler_fn(machine_check);
    idt.simd_floating_point.set_handler_fn(simd_floating_point);
    idt.virtualization.set_handler_fn(virtualization);
    idt.cp_protection_exception
        .set_handler_fn(cp_protection_exception);
    idt.hv_injection_exception
        .set_handler_fn(hv_injection_exception);
    idt.vmm_communication_exception
        .set_handler_fn(vmm_communication_exception);
    idt.security_exception.set_handler_fn(security_exception);
    idt[KEYBOARD_VECTOR].set_handler_fn(keyboard_interrupt);
    idt[MOUSE_VECTOR].set_handler_fn(mouse_interrupt);
    idt[SPURIOUS_MASTER].set_handler_fn(spurious_master);
    unsafe {
        idt[lapic::TIMER_VECTOR].set_handler_addr(VirtAddr::new(
            crate::sched::lapic_timer_entry as *const () as usize as u64,
        ));
    }
    idt[lapic::SPURIOUS_VECTOR].set_handler_fn(spurious_apic);
    idt
}

fn halt_exception(vec: u8) -> ! {
    crate::console::write("exception ");
    write_u8(vec);
    crate::hcf();
}

fn write_u8(n: u8) {
    if n >= 10 {
        let buf = [b'0' + n / 10, b'0' + n % 10];
        crate::console::write(core::str::from_utf8(&buf).unwrap());
    } else {
        let buf = [b'0' + n];
        crate::console::write(core::str::from_utf8(&buf).unwrap());
    }
}

extern "x86-interrupt" fn keyboard_interrupt(_frame: InterruptStackFrame) {
    ps2::irq();
    crate::kbd::drain_ps2();
    eoi(KEYBOARD_VECTOR);
    if crate::sched::current_is_zombie() {
        crate::sched::schedule_next();
    }
}

extern "x86-interrupt" fn mouse_interrupt(_frame: InterruptStackFrame) {
    ps2::irq();
    eoi(MOUSE_VECTOR);
}

extern "x86-interrupt" fn spurious_master(_frame: InterruptStackFrame) {
    eoi(SPURIOUS_MASTER);
}

extern "x86-interrupt" fn spurious_apic(_frame: InterruptStackFrame) {}

extern "x86-interrupt" fn divide_error(_frame: InterruptStackFrame) {
    halt_exception(0);
}
extern "x86-interrupt" fn debug(_frame: InterruptStackFrame) {
    halt_exception(1);
}
extern "x86-interrupt" fn non_maskable_interrupt(_frame: InterruptStackFrame) {
    halt_exception(2);
}
extern "x86-interrupt" fn breakpoint(_frame: InterruptStackFrame) {
    halt_exception(3);
}
extern "x86-interrupt" fn overflow(_frame: InterruptStackFrame) {
    halt_exception(4);
}
extern "x86-interrupt" fn bound_range_exceeded(_frame: InterruptStackFrame) {
    halt_exception(5);
}
extern "x86-interrupt" fn invalid_opcode(_frame: InterruptStackFrame) {
    halt_exception(6);
}
extern "x86-interrupt" fn device_not_available(_frame: InterruptStackFrame) {
    halt_exception(7);
}
extern "x86-interrupt" fn double_fault(_frame: InterruptStackFrame, _error: u64) -> ! {
    crate::console::write("double fault");
    crate::hcf();
}
extern "x86-interrupt" fn invalid_tss(_frame: InterruptStackFrame, _error: u64) {
    halt_exception(10);
}
extern "x86-interrupt" fn segment_not_present(_frame: InterruptStackFrame, _error: u64) {
    halt_exception(11);
}
extern "x86-interrupt" fn stack_segment_fault(_frame: InterruptStackFrame, _error: u64) {
    halt_exception(12);
}
extern "x86-interrupt" fn general_protection_fault(frame: InterruptStackFrame, _error: u64) {
    if frame.code_segment.rpl() == PrivilegeLevel::Ring3 {
        crate::process::return_to_kernel(crate::process::Outcome::Fault);
    }
    halt_exception(13);
}
extern "x86-interrupt" fn page_fault(frame: InterruptStackFrame, _error: PageFaultErrorCode) {
    if frame.code_segment.rpl() == PrivilegeLevel::Ring3 {
        crate::process::return_to_kernel(crate::process::Outcome::Fault);
    }
    halt_exception(14);
}
extern "x86-interrupt" fn x87_floating_point(_frame: InterruptStackFrame) {
    halt_exception(16);
}
extern "x86-interrupt" fn alignment_check(_frame: InterruptStackFrame, _error: u64) {
    halt_exception(17);
}
extern "x86-interrupt" fn machine_check(_frame: InterruptStackFrame) -> ! {
    halt_exception(18);
}
extern "x86-interrupt" fn simd_floating_point(_frame: InterruptStackFrame) {
    halt_exception(19);
}
extern "x86-interrupt" fn virtualization(_frame: InterruptStackFrame) {
    halt_exception(20);
}
extern "x86-interrupt" fn cp_protection_exception(_frame: InterruptStackFrame, _error: u64) {
    halt_exception(21);
}
extern "x86-interrupt" fn hv_injection_exception(_frame: InterruptStackFrame) {
    halt_exception(28);
}
extern "x86-interrupt" fn vmm_communication_exception(_frame: InterruptStackFrame, _error: u64) {
    halt_exception(29);
}
extern "x86-interrupt" fn security_exception(_frame: InterruptStackFrame, _error: u64) {
    halt_exception(30);
}
