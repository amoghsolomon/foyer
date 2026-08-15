//! Visual tokens for the native presentation surface.
//!
//! The palette intentionally stays monochrome. Meaning comes from hierarchy, opacity, spacing,
//! and motion; the narrator's active focus is the single high-contrast white outline.

pub(crate) const BACKGROUND: u32 = 0x0b0b0c;
pub(crate) const SURFACE: u32 = 0x18181a;
pub(crate) const SURFACE_RAISED: u32 = 0x202023;
pub(crate) const SURFACE_RECESSED: u32 = 0x111113;
pub(crate) const BORDER: u32 = 0x303034;
pub(crate) const FOREGROUND: u32 = 0xf1f1f2;
pub(crate) const MUTED: u32 = 0x96969d;
pub(crate) const SUBTLE: u32 = 0x696970;
pub(crate) const FOCUS: u32 = 0xffffff;

pub(crate) const CARD_RADIUS: f32 = 22.0;
pub(crate) const CARD_PADDING: f32 = 24.0;
pub(crate) const GRID_GAP: f32 = 14.0;
