//! Shared palette and layout tokens. `no_std`, no alloc.
//!
//! GUI chrome uses the grayscale atlas in `coeleo-draw`. The VT stays Flanterm.
//! `DECO_H` is the window title bar, not the file-manager toolbar.

#![no_std]

pub const GRID: u32 = 16;
pub const PANEL_H: u32 = GRID * 2;
pub const DECO_H: u32 = GRID * 2;
pub const SHADOW_PX: u32 = 8;
pub const PANEL_ALPHA: u8 = 218;
pub const PANEL_MARGIN: u32 = 7;
/// Inner padding on all four sides of the floating bar (slots, hover, pill).
pub const PANEL_INSET: u32 = 4;
pub const PAD: u32 = 8;
pub const GAP: u32 = 8;
pub const RADIUS: u32 = 6;
pub const RADIUS_SM: u32 = 4;
pub const TASK_ICON: u32 = 36;
pub const SHADOW_OFF: u32 = 2;
pub const SHADOW_A: [u8; 8] = [40, 28, 20, 14, 10, 6, 4, 2];

pub const BG: u32 = 0x001B_1B1B;
pub const SURFACE: u32 = 0x0036_3636;
pub const PANEL_BG: u32 = 0x0012_151A;
pub const ACCENT: u32 = 0x003D_AEE9;
pub const HIGHLIGHT: u32 = 0x0024_6AA0;
pub const HOVER: u32 = 0x0034_3A45;
pub const TEXT: u32 = 0x00E0_E0E0;
pub const DIM: u32 = 0x00A0_A0A0;
pub const DANGER: u32 = 0x00C0_394B;
pub const SHADOW: u32 = 0x0000_0000;
pub const OVERLAY: u32 = 0x0000_0000;
pub const OVERLAY_ALPHA: u8 = 102;
