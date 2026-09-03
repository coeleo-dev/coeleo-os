//! Shared virtio PCI probe. Finds one function of a given device type.

use virtio_drivers::transport::DeviceType;
use virtio_drivers::transport::pci::bus::{Command, PciRoot};
use virtio_drivers::transport::pci::{PciTransport, virtio_device_type};

use crate::pci::CamCf8;
use crate::virtio_hal::VirtioHal;

pub fn open(kind: DeviceType) -> Option<PciTransport> {
    let mut root = PciRoot::new(CamCf8);
    let mut found = None;
    'buses: for bus in 0u8..=31 {
        for (df, info) in root.enumerate_bus(bus) {
            if virtio_device_type(&info) == Some(kind) {
                found = Some(df);
                break 'buses;
            }
        }
    }
    let df = found?;

    let (_, mut cmd) = root.get_status_command(df);
    cmd.insert(Command::MEMORY_SPACE | Command::BUS_MASTER);
    root.set_command(df, cmd);

    PciTransport::new::<VirtioHal, _>(&mut root, df).ok()
}
