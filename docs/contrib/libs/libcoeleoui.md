# libcoeleoui

Immediate-mode widgets for GUI ELFs. `no_std`, **no** `alloc`. Paints via [coeleo-draw](coeleo-draw.md) with tokens from [coeleo-theme](coeleo-theme.md). Input via [libcoeleo](libcoeleo.md) (`poll_input`) fed into `Ui::feed`.

**Folder:** `userspace/libs/libcoeleoui`  
**Userspace workspace:** yes  
**Consumers:** `userspace/apps/widgets`, `userspace/apps/install`. The kernel compositor **does not** use this crate (it paints chrome separately).

## Dependencies

`libcoeleo`, `coeleo-theme`, `coeleo-draw` (sibling paths under `libs/`).

## `Ui`

```text
Ui::new() -> Ui
begin(&mut self, buf: &mut [u32], w, h)   // clear with BG; dirty on the first frame
label(x, y, s)
label_color(x, y, s, color)
button(x, y, w, h, s) -> bool              // true on click (KIND_DOWN in the rect)
button_fill(x, y, w, h, s, fill) -> bool   // `button` with a fill colour (use `DANGER` for wipe)
icon_button(x, y, Icon) -> bool            // 24×24
tooltip(x, y, s)
hovering(x, y, w, h) -> bool
key() -> Option<u8>                        // last KIND_KEY this frame (cleared in `end`)
down_outside(x, y, w, h) -> bool           // click not in the rect
text_field(x, y, w, buf: &mut [u8]) -> bool  // alias of search_field
search_field(x, y, w, buf) -> bool
toolbar(x, y, w, h)
disc(x, y, d, color)                       // step dots
list_row(x, y, w, s, sel) -> bool
end(&mut self) -> bool                     // true if win_damage is needed
feed(&mut self, bytes: &[u8])             // 16-byte events (kind, x, y, …)
```

`begin` expects a `u32` pixel buffer (same layout as `win_create`). `end` returns whether there was dirty; the app then calls `win_damage`.

Events in `feed`: every 16 bytes, LE `kind` in `[0..4]` (`1` = move, `2` = down, `3` = key), `x`/`y` as LE `i32`, `key` as LE `u32`. Key codes match the compositor (`KEY_ESC` = 6, `KEY_ENTER` = 4, arrows 2/3/8/9). Negative coordinates are ignored for mouse. This is what `poll_input` returns in the kernel.

## Do not

- Do not allocate. No `Vec` in this crate.
- Do not paint window shadow here — that is the compositor.
- Do not port Qt/egui/Slint to “complete” the toolkit.
