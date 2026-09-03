fn main() {
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    println!("cargo:rustc-link-arg=-Tlinker-{arch}.ld");
    println!("cargo:rerun-if-changed=linker-{arch}.ld");

    compile_flanterm();
    compile_stb_image();
}

/// Compile vendored Flanterm (v3.0.2) as freestanding x86_64 objects.
///
/// `-mcmodel=kernel` matches `x86_64-unknown-none` (higher-half). SSE is
/// disabled so the C code cannot emit instructions the kernel has not enabled.
fn compile_flanterm() {
    println!("cargo:rerun-if-changed=vendor/flanterm/src/flanterm.c");
    println!("cargo:rerun-if-changed=vendor/flanterm/src/flanterm.h");
    println!("cargo:rerun-if-changed=vendor/flanterm/src/flanterm_backends/fb.c");

    let mut build = cc::Build::new();
    // There is no `x86_64-unknown-none-gcc`; the host compiler emits ELF64
    // with the flags below.
    if let Ok(cc) = std::env::var("CC") {
        build.compiler(cc);
    } else {
        build.compiler("gcc");
    }

    build
        .files([
            "vendor/flanterm/src/flanterm.c",
            "vendor/flanterm/src/flanterm_backends/fb.c",
        ])
        .include("vendor/flanterm/src")
        .flag("-std=gnu11")
        .flag("-ffreestanding")
        .flag("-fno-stack-protector")
        .flag("-fno-stack-check")
        .flag("-fno-pic")
        .flag("-fno-pie")
        .flag("-mno-red-zone")
        .flag("-m64")
        .flag("-mcmodel=kernel")
        .flag("-mno-mmx")
        .flag("-mno-sse")
        .flag("-mno-sse2")
        .flag("-fno-asynchronous-unwind-tables")
        .warnings(false)
        .compile("flanterm");
}

fn kernel_cc() -> cc::Build {
    let mut build = cc::Build::new();
    if let Ok(cc) = std::env::var("CC") {
        build.compiler(cc);
    } else {
        build.compiler("gcc");
    }
    build
        .flag("-std=gnu11")
        .flag("-ffreestanding")
        .flag("-fno-stack-protector")
        .flag("-fno-stack-check")
        .flag("-fno-pic")
        .flag("-fno-pie")
        .flag("-mno-red-zone")
        .flag("-m64")
        .flag("-mcmodel=kernel")
        .flag("-mno-mmx")
        .flag("-mno-sse")
        .flag("-mno-sse2")
        .flag("-fno-asynchronous-unwind-tables")
        .warnings(false);
    build
}

fn compile_stb_image() {
    println!("cargo:rerun-if-changed=vendor/stb/stb_image.c");
    println!("cargo:rerun-if-changed=vendor/stb/stb_image.h");
    kernel_cc()
        .file("vendor/stb/stb_image.c")
        .include("vendor/stb")
        .compile("stb_image");
}
