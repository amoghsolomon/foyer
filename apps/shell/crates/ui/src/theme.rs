use gpui::{App, Hsla, px, rgb};
use gpui_component::theme::{Theme, ThemeMode};
use std::sync::Arc;

/// Fixed visual tokens shared by every Foyer Shell surface.
pub mod tokens {
    pub const BACKGROUND: u32 = 0x0b0b0c;
    pub const SURFACE: u32 = 0x18181a;
    pub const SURFACE_RAISED: u32 = 0x202023;
    pub const SURFACE_RECESSED: u32 = 0x111113;
    pub const BORDER: u32 = 0x303034;
    pub const FOREGROUND: u32 = 0xf1f1f2;
    pub const MUTED: u32 = 0x96969d;
    pub const SUBTLE: u32 = 0x696970;
    pub const FOCUS: u32 = 0xffffff;

    pub const TOOLBAR_WIDTH: f32 = 46.0;
    pub const PANEL_WIDTH: f32 = 440.0;
    pub const CARD_RADIUS: f32 = 22.0;
    pub const CONTROL_RADIUS: f32 = 14.0;
    pub const CARD_PADDING: f32 = 24.0;
    pub const GRID_GAP: f32 = 14.0;
}

/// Installs gpui-component and folds it into Foyer Shell's quiet monochrome palette.
pub fn init(cx: &mut App) {
    gpui_component::init(cx);
    Theme::change(ThemeMode::Dark, None, cx);
    let theme = Theme::global_mut(cx);
    theme.font_family = ".SystemUIFont".into();
    theme.mono_font_family = "DejaVu Sans Mono".into();
    theme.font_size = px(16.0);
    theme.mono_font_size = px(14.0);
    theme.radius = px(tokens::CONTROL_RADIUS);
    theme.radius_lg = px(tokens::CARD_RADIUS);
    theme.shadow = false;
    let background: Hsla = rgb(0x09090b).into();
    let surface: Hsla = rgb(0x18181b).into();
    let raised: Hsla = rgb(0x202024).into();
    let border: Hsla = rgb(0x303036).into();
    let foreground: Hsla = rgb(0xf4f4f5).into();
    let muted: Hsla = rgb(0xa1a1aa).into();
    let quiet: Hsla = rgb(0x71717a).into();

    theme.background = background;
    theme.foreground = foreground;
    theme.border = border;
    theme.input = border;
    theme.ring = foreground;
    theme.caret = foreground;
    theme.selection = Hsla::from(rgb(0xffffff)).opacity(0.18);
    theme.muted = raised;
    theme.muted_foreground = muted;
    theme.accent = raised;
    theme.accent_foreground = foreground;
    theme.primary = foreground;
    theme.primary_foreground = background;
    theme.colors.list = surface;
    theme.list_even = surface;
    theme.list_head = raised;
    theme.list_hover = raised;
    theme.list_active = raised;
    theme.list_active_border = foreground;
    theme.sidebar = surface;
    theme.sidebar_border = border;
    theme.sidebar_foreground = muted;
    theme.sidebar_accent = raised;
    theme.sidebar_accent_foreground = foreground;
    theme.popover = surface;
    theme.popover_foreground = foreground;
    theme.scrollbar = background;
    theme.scrollbar_thumb = border;
    theme.scrollbar_thumb_hover = quiet;
    theme.chart_1 = foreground;
    theme.chart_2 = rgb(0xd4d4d8).into();
    theme.chart_3 = rgb(0xa1a1aa).into();
    theme.chart_4 = rgb(0x71717a).into();
    theme.chart_5 = rgb(0x52525b).into();
    theme.chart_bullish = rgb(0xe4e4e7).into();
    theme.chart_bearish = rgb(0x71717a).into();
    theme.tokens = (&theme.colors).into();

    let mut highlight_theme = (*theme.highlight_theme).clone();
    highlight_theme.style.editor_active_line = None;
    highlight_theme.style.editor_active_line_number = None;
    theme.highlight_theme = Arc::new(highlight_theme);
}
