# coeleo-image

JPEG/PNG decode for wallpaper and file-manager preview. `no_std` **with** `alloc`.

**Folder:** `userspace/libs/coeleo-image`  
**Userspace workspace:** **no**  
**Consumers:** kernel only (`desk`, FM). Path in [`kernel/Cargo.toml`](../../../kernel/Cargo.toml).

Including this crate in [`userspace/Cargo.toml`](../../../userspace/Cargo.toml) with `build-std` duplicates `core` (`zune`). See [troubleshooting](../troubleshooting.md).

## Why JPEG via C

PNG: `zune-png`. JPEG: `stb_image` compiled in the **kernel** (`cc`, `-mno-sse`). `zune-jpeg` on `x86_64-unknown-none` can hang even on a small baseline; that is why JPEG does not go through zune.

## API

```text
MAX_BYTES = 1536 * 1024
MAX_SIDE  = 2048

decode_rgba(bytes) -> Result<(u32, u32, Vec<u32>), ()>
scale_nn(src, sw, sh, dw, dh) -> Result<Vec<u32>, ()>
scale_box(src, sw, sh, dw, dh) -> Result<Vec<u32>, ()>
```

`decode_rgba` chooses JPEG if the magic is `FF D8`, PNG if it is the PNG signature; any other format → `Err(())`. Pixels packed B,G,R,A in `u32`.

`scale_box` area-averages when shrinking; upscale falls back to `scale_nn`.

## Do not

- Do not add the crate to the userspace workspace.
- Do not switch JPEG to zune without an acceptance test that proves it does not hang on the none target.
- Do not raise `MAX_*` “just in case” — the kernel heap is the real limit.
