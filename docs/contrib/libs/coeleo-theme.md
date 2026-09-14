# coeleo-theme

Shared palette and layout tokens. `no_std`, no `alloc`, no dependencies.

**Folder:** `userspace/libs/coeleo-theme`  
**Userspace workspace:** yes  
**Consumers:** kernel (panel, compositor, FM), [coeleo-draw](coeleo-draw.md), [libcoeleoui](libcoeleoui.md).

The GUI uses the atlas in `coeleo-draw`. The VT stays Flanterm. `DECO_H` is the window title bar, not the file-manager toolbar.

## Layout constants

| Name | Value | Notes |
| --- | --- | --- |
| `GRID` | 16 | grid (VT 8×16 bitmap font height) |
| `PANEL_H` | 32 | `GRID * 2` |
| `DECO_H` | 32 | title bar |
| `DECO_BTN` | 24 | min/max/close; not full `DECO_H` |
| `DECO_BTN_PAD` | 4 | vertical inset under the focus strip |
| `BUTTON_H` | 24 | compact push / icon button |
| `SHADOW_PX` | 8 | shadow ring |
| `PANEL_ALPHA` | 218 | panel alpha |
| `PANEL_MARGIN` | 7 | floating panel margin |
| `PANEL_INSET` | 4 | inner padding of the floating bar |
| `PAD` | 8 | |
| `GAP` | 8 | |
| `RADIUS` | 6 | corners |
| `RADIUS_SM` | 4 | pills, rows, small buttons |
| `TASK_ICON` | 36 | |
| `SHADOW_OFF` | 2 | |
| `SHADOW_A` | `[40, 28, 20, 14, 10, 6, 4, 2]` | 8 alpha levels |

## Colors (`u32`, 0x00RRGGBB in the typical framebuffer high nibble)

`BG`, `SURFACE`, `PANEL_BG`, `ACCENT` (`0x003D_AEE9`), `HIGHLIGHT`, `HOVER`, `TEXT`, `DIM`, `DANGER`, `SHADOW`.

No functions. Only `pub const`.

## Do not

- Do not put color literals in the compositor or FM if the token already exists.
- Do not create a `kernel-ui` crate “so the theme can leave userspace”.
- Do not change tokens without visual acceptance (Plasma tests / phase 13).
