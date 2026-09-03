//! PCI configuration access via ports 0xCF8 / 0xCFC.

use spin::Mutex;
use virtio_drivers::transport::pci::bus::{ConfigurationAccess, DeviceFunction};
use x86_64::instructions::port::Port;

static CAM: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy)]
pub struct CamCf8;

fn address(df: DeviceFunction, offset: u8) -> u32 {
    0x8000_0000
        | (u32::from(df.bus) << 16)
        | (u32::from(df.device) << 11)
        | (u32::from(df.function) << 8)
        | (u32::from(offset) & 0xFC)
}

impl ConfigurationAccess for CamCf8 {
    fn read_word(&self, device_function: DeviceFunction, register_offset: u8) -> u32 {
        let addr = address(device_function, register_offset);
        let _guard = CAM.lock();
        // SAFETY: CAM mutex serializes 0xCF8/0xCFC; kernel-only I/O.
        unsafe {
            Port::<u32>::new(0xCF8).write(addr);
            Port::<u32>::new(0xCFC).read()
        }
    }

    fn write_word(&mut self, device_function: DeviceFunction, register_offset: u8, data: u32) {
        let addr = address(device_function, register_offset);
        let _guard = CAM.lock();
        // SAFETY: same as read_word.
        unsafe {
            Port::<u32>::new(0xCF8).write(addr);
            Port::<u32>::new(0xCFC).write(data);
        }
    }

    unsafe fn unsafe_clone(&self) -> Self {
        *self
    }
}
