//! Slot enable, Address Device, control/bulk/interrupt transfers.

use core::sync::atomic::{Ordering, compiler_fence};

use x86_64::{PhysAddr, VirtAddr};

use super::host::{
    self, CC_SHORT, CC_STALL, CC_SUCCESS, CH, CYCLE, DIR_IN, Host, IDT, IOC, IntEp, Kind, MAX_SLOTS,
    Slot, TRB_ADDRESS, TRB_CONFIG_EP, TRB_DATA, TRB_DISABLE_SLOT, TRB_ENABLE_SLOT, TRB_EVAL,
    TRB_NORMAL, TRB_SETUP, TRB_STATUS, TRT_IN, TRT_OUT,
};
use super::Setup;
use crate::pmm;

pub(super) fn disable_slot(hc: &mut Host, slot: u8) {
    host::enqueue_cmd(
        hc,
        0,
        0,
        0,
        host::trb(CYCLE, TRB_DISABLE_SLOT, u32::from(slot) << 24),
    );
    host::doorbell(hc, 0, 0);
    let _ = host::wait_cmd(hc);
    if (slot as usize) < hc.slots.len() {
        hc.slots[slot as usize] = None;
    }
}

pub(super) fn enable_slot(hc: &mut Host) -> Option<u8> {
    host::enqueue_cmd(hc, 0, 0, 0, host::trb(CYCLE, TRB_ENABLE_SLOT, 0));
    host::doorbell(hc, 0, 0);
    let ev = host::wait_cmd(hc)?;
    if ev.code != CC_SUCCESS || ev.slot == 0 || ev.slot > MAX_SLOTS {
        return None;
    }
    let slot = ev.slot;
    let out_page = host::page()?;
    let ep0_page = host::page()?;
    unsafe {
        hc.dcbaa
            .as_mut_ptr::<u8>()
            .add(slot as usize * 8)
            .cast::<u64>()
            .write_volatile(out_page.p.as_u64());
    }
    hc.slots[slot as usize] = Some(Slot {
        kind: Kind::Unknown,
        speed: 0,
        root_port: 0,
        route: 0,
        parent: 0,
        parent_port: 0,
        out_ctx: out_page.v,
        out_ctx_phys: out_page.p,
        ep0: host::ring_init(ep0_page.v, ep0_page.p),
        bulk_out: None,
        bulk_in: None,
        dci_out: 0,
        dci_in: 0,
        int_eps: [None, None],
    });
    Some(slot)
}

pub(super) fn address_device(
    hc: &mut Host,
    slot: u8,
    root_port: u8,
    speed: u8,
    route: u32,
    parent: u8,
    parent_port: u8,
) -> bool {
    let ep0_phys = {
        let Some(dev) = hc.slots[slot as usize].as_mut() else {
            return false;
        };
        dev.speed = speed;
        dev.root_port = root_port;
        dev.route = route;
        dev.parent = parent;
        dev.parent_port = parent_port;
        host::zero(dev.out_ctx, pmm::FRAME_SIZE as usize);
        dev.ep0.phys
    };
    host::zero(hc.in_ctx, pmm::FRAME_SIZE as usize);
    let max0 = host::max_packet0(speed);
    let sz = hc.ctx_sz;
    host::write_ctx(hc.in_ctx, sz, 0, 1, 0x3);
    let s0: u32 = (route & 0xF_FFFF) | (u32::from(speed) << 20) | (1 << 27);
    host::write_ctx(hc.in_ctx, sz, 1, 0, s0);
    host::write_ctx(hc.in_ctx, sz, 1, 1, u32::from(root_port) << 16);
    if parent != 0 && speed <= 2 {
        let parent_speed = hc
            .slots
            .get(parent as usize)
            .and_then(|s| s.as_ref())
            .map(|s| s.speed)
            .unwrap_or(0);
        if parent_speed == 3 {
            host::write_ctx(
                hc.in_ctx,
                sz,
                1,
                2,
                u32::from(parent) | (u32::from(parent_port) << 8),
            );
        }
    }
    host::write_ctx(
        hc.in_ctx,
        sz,
        2,
        1,
        (3 << 1) | (4 << 3) | (u32::from(max0) << 16),
    );
    let dq = ep0_phys.as_u64() | 1;
    host::write_ctx(hc.in_ctx, sz, 2, 2, dq as u32);
    host::write_ctx(hc.in_ctx, sz, 2, 3, (dq >> 32) as u32);
    compiler_fence(Ordering::SeqCst);

    host::enqueue_cmd(
        hc,
        hc.in_ctx_phys.as_u64(),
        0,
        0,
        host::trb(CYCLE, TRB_ADDRESS, u32::from(slot) << 24),
    );
    host::doorbell(hc, 0, 0);
    let Some(ev) = host::wait_cmd(hc) else {
        return false;
    };
    ev.code == CC_SUCCESS
}

pub(super) fn eval_hub(hc: &mut Host, slot: u8, nports: u8) -> bool {
    if hc.slots[slot as usize].is_none() {
        return false;
    }
    let sz = hc.ctx_sz;
    host::zero(hc.in_ctx, pmm::FRAME_SIZE as usize);
    copy_slot_ctx(hc, slot);
    let s0 = host::read_ctx(hc.in_ctx, sz, 1, 0) | (1 << 26);
    host::write_ctx(hc.in_ctx, sz, 1, 0, s0);
    let s1 = (host::read_ctx(hc.in_ctx, sz, 1, 1) & 0x00FF_FFFF) | (u32::from(nports) << 24);
    host::write_ctx(hc.in_ctx, sz, 1, 1, s1);
    host::write_ctx(hc.in_ctx, sz, 0, 1, 0x1);
    compiler_fence(Ordering::SeqCst);
    host::enqueue_cmd(
        hc,
        hc.in_ctx_phys.as_u64(),
        0,
        0,
        host::trb(CYCLE, TRB_EVAL, u32::from(slot) << 24),
    );
    host::doorbell(hc, 0, 0);
    host::wait_cmd(hc).is_some_and(|e| e.code == CC_SUCCESS)
}

pub(super) fn config_bulk(
    hc: &mut Host,
    slot: u8,
    ep_out: u8,
    ep_in: u8,
    max_out: u16,
    max_in: u16,
) -> Result<(), ()> {
    let dci_out = host::dci(ep_out);
    let dci_in = host::dci(ep_in);
    if dci_out < 2 || dci_in < 2 {
        return Err(());
    }
    let out_page = host::page().ok_or(())?;
    let in_page = host::page().ok_or(())?;
    let out_ring = host::ring_init(out_page.v, out_page.p);
    let in_ring = host::ring_init(in_page.v, in_page.p);
    let sz = hc.ctx_sz;
    host::zero(hc.in_ctx, pmm::FRAME_SIZE as usize);
    copy_slot_ctx(hc, slot);
    let max_dci = dci_out.max(dci_in);
    let s0 = host::read_ctx(hc.in_ctx, sz, 1, 0) & !(0x1F << 27);
    host::write_ctx(hc.in_ctx, sz, 1, 0, s0 | (u32::from(max_dci) << 27));
    let add = 1u32 | (1 << dci_out) | (1 << dci_in);
    host::write_ctx(hc.in_ctx, sz, 0, 1, add);
    fill_ep(hc, dci_out, 2, max_out, out_ring.phys, 0);
    fill_ep(hc, dci_in, 6, max_in, in_ring.phys, 0);
    compiler_fence(Ordering::SeqCst);
    host::enqueue_cmd(
        hc,
        hc.in_ctx_phys.as_u64(),
        0,
        0,
        host::trb(CYCLE, TRB_CONFIG_EP, u32::from(slot) << 24),
    );
    host::doorbell(hc, 0, 0);
    let ev = host::wait_cmd(hc).ok_or(())?;
    if ev.code != CC_SUCCESS {
        return Err(());
    }
    let dev = hc.slots[slot as usize].as_mut().ok_or(())?;
    dev.bulk_out = Some(out_ring);
    dev.bulk_in = Some(in_ring);
    dev.dci_out = dci_out;
    dev.dci_in = dci_in;
    Ok(())
}

pub(super) struct IntSpec {
    pub ep: u8,
    pub maxpkt: u16,
}

pub(super) fn config_interrupt(hc: &mut Host, slot: u8, specs: &[IntSpec]) -> Result<(), ()> {
    if specs.is_empty() || specs.len() > 2 {
        return Err(());
    }
    let mut prepared: [Option<(u8, host::Ring, VirtAddr, PhysAddr, u16)>; 2] = [None, None];
    let mut add = 1u32;
    let mut max_dci = 1u8;
    for (i, spec) in specs.iter().enumerate() {
        let dci = host::dci(spec.ep);
        if dci < 2 {
            return Err(());
        }
        let rp = host::page().ok_or(())?;
        let bp = host::page().ok_or(())?;
        prepared[i] = Some((
            dci,
            host::ring_init(rp.v, rp.p),
            bp.v,
            bp.p,
            spec.maxpkt.max(8),
        ));
        add |= 1 << dci;
        max_dci = max_dci.max(dci);
    }
    let sz = hc.ctx_sz;
    host::zero(hc.in_ctx, pmm::FRAME_SIZE as usize);
    copy_slot_ctx(hc, slot);
    let s0 = host::read_ctx(hc.in_ctx, sz, 1, 0) & !(0x1F << 27);
    host::write_ctx(hc.in_ctx, sz, 1, 0, s0 | (u32::from(max_dci) << 27));
    host::write_ctx(hc.in_ctx, sz, 0, 1, add);
    for item in prepared.iter().flatten() {
        fill_ep(hc, item.0, 7, item.4, item.1.phys, 8);
    }
    compiler_fence(Ordering::SeqCst);
    host::enqueue_cmd(
        hc,
        hc.in_ctx_phys.as_u64(),
        0,
        0,
        host::trb(CYCLE, TRB_CONFIG_EP, u32::from(slot) << 24),
    );
    host::doorbell(hc, 0, 0);
    let ev = host::wait_cmd(hc).ok_or(())?;
    if ev.code != CC_SUCCESS {
        return Err(());
    }
    let dev = hc.slots[slot as usize].as_mut().ok_or(())?;
    for (i, item) in prepared.iter_mut().enumerate() {
        let Some((dci, ring, buf, buf_phys, maxpkt)) = item.take() else {
            continue;
        };
        dev.int_eps[i] = Some(IntEp {
            dci,
            ring,
            buf,
            buf_phys,
            maxpkt,
            armed: false,
        });
    }
    Ok(())
}

fn fill_ep(hc: &Host, dci: u8, ep_type: u32, maxpkt: u16, ring: PhysAddr, interval: u8) {
    let idx = 1 + dci;
    let sz = hc.ctx_sz;
    if interval != 0 {
        host::write_ctx(hc.in_ctx, sz, idx, 0, u32::from(interval) << 16);
    }
    host::write_ctx(
        hc.in_ctx,
        sz,
        idx,
        1,
        (3 << 1) | (ep_type << 3) | (u32::from(maxpkt) << 16),
    );
    let dq = ring.as_u64() | 1;
    host::write_ctx(hc.in_ctx, sz, idx, 2, dq as u32);
    host::write_ctx(hc.in_ctx, sz, idx, 3, (dq >> 32) as u32);
    host::write_ctx(hc.in_ctx, sz, idx, 4, u32::from(maxpkt));
}

fn copy_slot_ctx(hc: &Host, slot: u8) {
    let Some(dev) = hc.slots[slot as usize].as_ref() else {
        return;
    };
    let sz = hc.ctx_sz;
    for i in 0..sz {
        unsafe {
            hc.in_ctx
                .as_mut_ptr::<u8>()
                .add(sz + i)
                .write(dev.out_ctx.as_ptr::<u8>().add(i).read());
        }
    }
}

pub(super) fn control_inner(
    hc: &mut Host,
    slot: u8,
    setup: Setup,
    data: &mut [u8],
) -> Result<usize, ()> {
    let n = setup.len as usize;
    if n > pmm::FRAME_SIZE as usize {
        return Err(());
    }
    if hc.slots[slot as usize].is_none() {
        return Err(());
    }
    let inp = setup.ty & 0x80 != 0;
    let has = n != 0;
    if has && !inp {
        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr(), hc.bounce.as_mut_ptr::<u8>(), n);
        }
    }
    let pkt = [
        setup.ty,
        setup.req,
        setup.value as u8,
        (setup.value >> 8) as u8,
        setup.index as u8,
        (setup.index >> 8) as u8,
        setup.len as u8,
        (setup.len >> 8) as u8,
    ];
    let s0 = u32::from_le_bytes(pkt[0..4].try_into().unwrap_or([0; 4]));
    let s1 = u32::from_le_bytes(pkt[4..8].try_into().unwrap_or([0; 4]));
    let trt = if !has {
        0
    } else if inp {
        TRT_IN
    } else {
        TRT_OUT
    };
    let bounce = hc.bounce_phys;
    {
        let dev = hc.slots[slot as usize].as_mut().ok_or(())?;
        host::enqueue_ep(
            &mut dev.ep0,
            s0 as u64 | ((s1 as u64) << 32),
            8,
            host::trb(CYCLE, TRB_SETUP, IDT | CH | trt),
        );
        if has {
            let dir = if inp { DIR_IN } else { 0 };
            host::enqueue_ep(
                &mut dev.ep0,
                bounce.as_u64(),
                n as u32,
                host::trb(CYCLE, TRB_DATA, CH | dir),
            );
        }
        let sdir = if has && inp { 0 } else { DIR_IN };
        host::enqueue_ep(
            &mut dev.ep0,
            0,
            0,
            host::trb(CYCLE, TRB_STATUS, IOC | sdir),
        );
    }
    compiler_fence(Ordering::SeqCst);
    host::doorbell(hc, slot, 1);
    let ev = host::wait_xfer(hc, slot, 1).ok_or(())?;
    if ev.code != CC_SUCCESS && ev.code != CC_SHORT && ev.code != CC_STALL {
        return Err(());
    }
    if ev.code == CC_STALL {
        return Err(());
    }
    if has && inp {
        let got = n.saturating_sub(ev.remain as usize).min(data.len());
        unsafe {
            core::ptr::copy_nonoverlapping(hc.bounce.as_ptr::<u8>(), data.as_mut_ptr(), got);
        }
        return Ok(got);
    }
    Ok(n)
}

pub(super) fn bounce_out(hc: &mut Host, slot: u8, buf: &[u8]) -> Result<(), ()> {
    let mut off = 0;
    while off < buf.len() {
        let n = (buf.len() - off).min(pmm::FRAME_SIZE as usize);
        unsafe {
            core::ptr::copy_nonoverlapping(buf[off..].as_ptr(), hc.bounce.as_mut_ptr::<u8>(), n);
        }
        let dci = hc.slots[slot as usize].as_ref().ok_or(())?.dci_out;
        if dci == 0 {
            return Err(());
        }
        let bounce = hc.bounce_phys;
        {
            let ring = hc.slots[slot as usize]
                .as_mut()
                .ok_or(())?
                .bulk_out
                .as_mut()
                .ok_or(())?;
            host::enqueue_ep(ring, bounce.as_u64(), n as u32, host::trb(CYCLE, TRB_NORMAL, IOC));
        }
        compiler_fence(Ordering::SeqCst);
        host::doorbell(hc, slot, dci);
        let ev = host::wait_xfer(hc, slot, dci).ok_or(())?;
        if ev.code != CC_SUCCESS && ev.code != CC_SHORT {
            return Err(());
        }
        off += n;
    }
    Ok(())
}

pub(super) fn bounce_in(hc: &mut Host, slot: u8, buf: &mut [u8]) -> Result<(), ()> {
    let mut off = 0;
    while off < buf.len() {
        let n = (buf.len() - off).min(pmm::FRAME_SIZE as usize);
        let dci = hc.slots[slot as usize].as_ref().ok_or(())?.dci_in;
        if dci == 0 {
            return Err(());
        }
        let bounce = hc.bounce_phys;
        {
            let ring = hc.slots[slot as usize]
                .as_mut()
                .ok_or(())?
                .bulk_in
                .as_mut()
                .ok_or(())?;
            host::enqueue_ep(ring, bounce.as_u64(), n as u32, host::trb(CYCLE, TRB_NORMAL, IOC));
        }
        compiler_fence(Ordering::SeqCst);
        host::doorbell(hc, slot, dci);
        let ev = host::wait_xfer(hc, slot, dci).ok_or(())?;
        if ev.code != CC_SUCCESS && ev.code != CC_SHORT {
            return Err(());
        }
        let got = n.saturating_sub(ev.remain as usize).min(n);
        unsafe {
            core::ptr::copy_nonoverlapping(hc.bounce.as_ptr::<u8>(), buf[off..].as_mut_ptr(), got);
        }
        off += n;
    }
    Ok(())
}

pub(super) fn interrupt_poll(hc: &mut Host, slot: u8, dci: u8, buf: &mut [u8]) -> Option<usize> {
    host::pump(hc);
    let ep_i = {
        let dev = hc.slots[slot as usize].as_ref()?;
        dev.int_eps
            .iter()
            .position(|e| e.as_ref().is_some_and(|ep| ep.dci == dci))?
    };
    let armed = hc.slots[slot as usize]
        .as_ref()?
        .int_eps[ep_i]
        .as_ref()?
        .armed;
    if !armed {
        arm_int(hc, slot, ep_i);
        return None;
    }
    if host::take_xfer(hc, slot, dci).is_none() {
        return None;
    }
    let (src, n) = {
        let ep = hc.slots[slot as usize].as_ref()?.int_eps[ep_i].as_ref()?;
        let n = (ep.maxpkt as usize).min(buf.len()).min(64);
        (ep.buf, n)
    };
    unsafe {
        core::ptr::copy_nonoverlapping(src.as_ptr::<u8>(), buf.as_mut_ptr(), n);
    }
    if let Some(ep) = hc.slots[slot as usize]
        .as_mut()
        .and_then(|d| d.int_eps[ep_i].as_mut())
    {
        ep.armed = false;
    }
    arm_int(hc, slot, ep_i);
    Some(n)
}

fn arm_int(hc: &mut Host, slot: u8, ep_i: usize) {
    let dci;
    {
        let Some(dev) = hc.slots[slot as usize].as_mut() else {
            return;
        };
        let Some(ep) = dev.int_eps[ep_i].as_mut() else {
            return;
        };
        let maxpkt = u32::from(ep.maxpkt);
        let buf = ep.buf_phys;
        dci = ep.dci;
        host::enqueue_ep(
            &mut ep.ring,
            buf.as_u64(),
            maxpkt,
            host::trb(CYCLE, TRB_NORMAL, IOC),
        );
        ep.armed = true;
    }
    compiler_fence(Ordering::SeqCst);
    host::doorbell(hc, slot, dci);
}
