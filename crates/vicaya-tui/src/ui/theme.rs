//! Midnight Amber color theme for dark mode.

use ratatui::style::Color;

/// Background & Surface colors
pub const BG_DARK: Color = Color::Rgb(18, 18, 24); // #121218 - Deep dark blue
pub const BG_SURFACE: Color = Color::Rgb(24, 24, 32); // #181820 - Card background
pub const BG_ELEVATED: Color = Color::Rgb(32, 32, 42); // #20202A - Hover/selected

/// Primary - Amber/Orange (warm, easy on eyes)
pub const PRIMARY: Color = Color::Rgb(255, 179, 71); // #FFB347 - Amber
pub const PRIMARY_DIM: Color = Color::Rgb(220, 150, 50); // #DC9632 - Dimmed amber

/// Accent - Cyan (for highlights)
pub const ACCENT: Color = Color::Rgb(103, 224, 227); // #67E0E3 - Cyan
pub const ACCENT_DIM: Color = Color::Rgb(80, 180, 183); // #50B4B7 - Dimmed cyan

/// Text colors
pub const TEXT_PRIMARY: Color = Color::Rgb(230, 230, 235); // #E6E6EB - High contrast
pub const TEXT_SECONDARY: Color = Color::Rgb(160, 160, 170); // #A0A0AA - Secondary text
pub const TEXT_MUTED: Color = Color::Rgb(100, 100, 110); // #64646E - Muted text

/// Semantic colors
pub const SUCCESS: Color = Color::Rgb(118, 218, 133); // #76DA85 - Green
pub const WARNING: Color = Color::Rgb(255, 193, 94); // #FFC15E - Orange
pub const ERROR: Color = Color::Rgb(255, 108, 108); // #FF6C6C - Red
pub const INFO: Color = Color::Rgb(130, 170, 255); // #82AAFF - Blue

/// Border colors
pub const BORDER_DIM: Color = Color::Rgb(48, 48, 58); // #30303A - Subtle border
pub const BORDER_FOCUS: Color = PRIMARY; // Focus indicator

/// Get score-based color for search results
pub fn score_color(score: f32) -> Color {
    if score >= 0.9 {
        SUCCESS
    } else if score >= 0.7 {
        PRIMARY
    } else {
        TEXT_MUTED
    }
}
