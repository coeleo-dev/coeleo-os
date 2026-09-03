#![no_std]
#![no_main]

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    loop {
        let ms = libcoeleo::clock_ms();
        let n = ms / 1000;
        write_tick(n);
        let next = (n + 1) * 1000;
        while libcoeleo::clock_ms() < next {}
    }
}

fn write_tick(n: u64) {
    let mut buf = [0u8; 24];
    buf[0] = b't';
    buf[1] = b'i';
    buf[2] = b'c';
    buf[3] = b'k';
    buf[4] = b' ';
    let mut i = 5usize;
    if n == 0 {
        buf[i] = b'0';
        i += 1;
    } else {
        let mut tmp = [0u8; 20];
        let mut x = n;
        let mut t = 0usize;
        while x > 0 {
            tmp[t] = b'0' + (x % 10) as u8;
            t += 1;
            x /= 10;
        }
        while t > 0 {
            t -= 1;
            buf[i] = tmp[t];
            i += 1;
        }
    }
    buf[i] = b'\n';
    i += 1;
    let _ = libcoeleo::write(1, &buf[..i]);
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
