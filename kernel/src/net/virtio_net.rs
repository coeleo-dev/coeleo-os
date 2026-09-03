//! virtio-net: poll Ethernet NIC, smoltcp `phy::Device`. DMA stays in the HAL.

use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::time::Instant;
use virtio_drivers::device::net::VirtIONet;
use virtio_drivers::transport::DeviceType;
use virtio_drivers::transport::pci::PciTransport;

use crate::virtio_hal::VirtioHal;

const QUEUE_SIZE: usize = 16;
const BUF_LEN: usize = 2048;
const MTU: usize = 1514;

type Nic = VirtIONet<VirtioHal, PciTransport, QUEUE_SIZE>;

pub struct VirtioNetDev {
    nic: Nic,
}

pub struct RxTok {
    frame: [u8; BUF_LEN],
    len: usize,
}

pub struct TxTok<'a> {
    nic: &'a mut Nic,
}

impl VirtioNetDev {
    pub fn probe() -> Option<Self> {
        let transport = crate::virtio_pci::open(DeviceType::Network)?;
        let mut nic = Nic::new(transport, BUF_LEN).ok()?;
        nic.disable_interrupts();
        Some(Self { nic })
    }

    pub fn mac(&self) -> [u8; 6] {
        self.nic.mac_address()
    }
}

impl Device for VirtioNetDev {
    type RxToken<'a> = RxTok;
    type TxToken<'a> = TxTok<'a>;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        if !self.nic.can_recv() {
            return None;
        }
        let buf = self.nic.receive().ok()?;
        let pkt = buf.packet();
        let len = pkt.len().min(BUF_LEN);
        let mut frame = [0u8; BUF_LEN];
        frame[..len].copy_from_slice(&pkt[..len]);
        self.nic.recycle_rx_buffer(buf).ok()?;
        Some((RxTok { frame, len }, TxTok { nic: &mut self.nic }))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        if !self.nic.can_send() {
            return None;
        }
        Some(TxTok { nic: &mut self.nic })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = MTU;
        caps.max_burst_size = Some(1);
        caps.medium = Medium::Ethernet;
        caps
    }
}

impl RxToken for RxTok {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(&self.frame[..self.len])
    }
}

impl TxToken for TxTok<'_> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut tx = self.nic.new_tx_buffer(len);
        let result = f(tx.packet_mut());
        let _ = self.nic.send(tx);
        result
    }
}
