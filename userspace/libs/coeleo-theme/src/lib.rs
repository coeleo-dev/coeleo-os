//! Shared palette and layout tokens. `no_std`, no alloc.
//!
//! GUI chrome uses the grayscale atlas in `coeleo-draw`. The VT stays Flanterm.
//! `DECO_H` is the window title bar, not the file-manager toolbar.

#![no_std]

pub const GRID: u32 = 16;
pub const PANEL_H: u32 = GRID * 2;
pub const DECO_H: u32 = GRID * 2;
pub const SHADOW_PX: u32 = 8;
pub const PANEL_ALPHA: u8 = 224;
pub const PANEL_MARGIN: u32 = 7;
/// Inner padding on all four sides of the floating bar (slots, hover, pill).
pub const PANEL_INSET: u32 = 4;
pub const PAD: u32 = 8;
pub const GAP: u32 = 8;
pub const RADIUS: u32 = 8;
pub const RADIUS_SM: u32 = 5;
/// Window min/max/close control (not the full title-bar height).
pub const DECO_BTN: u32 = 24;
/// Vertical inset so deco hover sits below the 3 px focus strip.
pub const DECO_BTN_PAD: u32 = 4;
/// Compact Yaru-style push button / icon button.
pub const BUTTON_H: u32 = 24;
pub const TASK_ICON: u32 = 36;
pub const SHADOW_OFF: u32 = 2;
pub const SHADOW_A: [u8; 8] = [44, 30, 22, 16, 11, 7, 4, 2];

pub const BG: u32 = 0x000F_1218;
pub const SURFACE: u32 = 0x001C_212B;
pub const SURFACE_RAISED: u32 = 0x0024_2B38;
pub const PANEL_BG: u32 = 0x0014_1822;
pub const BORDER: u32 = 0x0030_3846;
pub const BORDER_LIGHT: u32 = 0x003D_4657;
pub const ACCENT: u32 = 0x0038_8BFD;
pub const HIGHLIGHT: u32 = 0x001F_6FEB;
pub const HOVER: u32 = 0x0028_303F;
pub const TEXT: u32 = 0x00F0_F6FC;
pub const DIM: u32 = 0x008B_949E;
pub const DANGER: u32 = 0x00F8_5149;
pub const SHADOW: u32 = 0x0000_0000;
pub const OVERLAY: u32 = 0x0005_070A;
pub const OVERLAY_ALPHA: u8 = 120;
