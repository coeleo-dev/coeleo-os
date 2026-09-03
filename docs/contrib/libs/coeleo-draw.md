# coeleo-draw

Immediate-mode paint primitives for kernel chrome and [libcoeleoui](libcoeleoui.md). `no_std`, no `alloc`.

**Folder:** `userspace/libs/coeleo-draw`  
**Userspace workspace:** yes  
**Consumers:** kernel (`ui/comp`, panel, …), `libcoeleoui`.

This is **not** the VT: Flanterm + `font8x16` in the kernel stay separate. The GUI font is a grayscale atlas (~11 px) generated at **build** time from `docs/fonts/IBMPlexMono-Regular.ttf`.

## Build

[`build.rs`](../../../userspace/libs/coeleo-draw/build.rs) (host, `fontdue`) rasterizes the TTF. Path from `CARGO_MANIFEST_DIR`:

`../../../docs/fonts/IBMPlexMono-Regular.ttf`

Moving the crate without updating this prefix breaks the build. See [troubleshooting](../troubleshooting.md).

Runtime dependency: [coeleo-theme](coeleo-theme.md).

## Types

```text
Target { addr: usize, w, h, pitch }   // framebuffer or client buffer
Clip { x0, y0, x1, y1 }               // Clip::all(w, h)
enum Icon { Folder, Terminal, Search, App, Close, Back, Forward }
```

Constants: `FONT_W` / `FONT_H` (atlas), `ICON` = 16.

`font` module: `coverage(c, gx, gy)` on the generated atlas (`OUT_DIR/font_atlas.rs`).

## Public functions (selection)

`put`, `get`, `fill_span`, `coverage_round`, `in_round`, `round_sdf`, `fill_round`, `fill_round_blend`, `hover_fill`, `highlight_fill`, `blend`, `shade_px`, `text`, `text_bold`, `tooltip_size`, `tooltip`, `icon_blit`, `icon`, `btn_shadow`, `dim`.

Clip is required on routines that paint rectangles/icons/text with clipping.

## Do not

- Do not draw the VT with this atlas.
- Do not pull `std` into the runtime crate (`build.rs` is host).
- Do not unify this `Target`/`Clip` with the compositor or KRunner `Rect` just because the name matches.
