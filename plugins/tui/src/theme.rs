//! Alumet's visual identity, distilled from its logo (`assets/logo.png`): a flame whose gradient
//! runs gold → orange → red, rising out of electric-cyan circuit traces. The UI borrows that pairing
//! — warm flame tones for branding, marks and alerts; cyan for live data and interaction — over a
//! dark base, so it feels like part of the Alumet world rather than a generic terminal app.
//!
//! Colors are true-color [`Color::Rgb`] so they match the logo exactly; terminals without 24-bit
//! support degrade them to their nearest palette entry, as the logo itself already does.

use ratatui::style::{Color, Modifier, Style};

// -- Flame gradient (the logo's mark) --------------------------------------------------------------

/// Flame tip — the brightest, most "Alumet" tone. Used for the brand title, the active tab and marks.
pub const GOLD: Color = Color::Rgb(0xFB, 0xB0, 0x1A);
/// Mid-flame orange, for secondary warm accents (e.g. deeper grouping levels).
pub const ORANGE: Color = Color::Rgb(0xF5, 0x82, 0x20);
/// Flame base — a hot red used for alerts: the detail-loss warning and the paused state.
pub const EMBER: Color = Color::Rgb(0xE6, 0x33, 0x29);

// -- Circuit (the logo's traces) -------------------------------------------------------------------

/// Electric cyan, the interaction/live-data accent: sparklines, sort arrows, focus, header emphasis.
pub const CYAN: Color = Color::Rgb(0x1B, 0xB8, 0xE6);
/// A dim cyan-tinted ink for the status bar background, echoing the circuit traces over a dark base.
pub const BAR_BG: Color = Color::Rgb(0x0C, 0x29, 0x32);
/// Background of the selected row — a darker cyan wash that reads as "here" without shouting.
pub const SELECTION_BG: Color = Color::Rgb(0x12, 0x3A, 0x47);

// -- Neutrals --------------------------------------------------------------------------------------

/// Primary text on the dark status bar.
pub const TEXT: Color = Color::Rgb(0xEC, 0xEC, 0xF0);
/// Secondary text (version string, log lines, idle tab labels).
pub const MUTED: Color = Color::Rgb(0x9A, 0xA0, 0xAC);
/// Faint text for the least important hints (overlay footnotes, "collecting…").
pub const FAINT: Color = Color::Rgb(0x5E, 0x66, 0x72);

/// Colors cycled through for graphed series and grouping depth — the brand quartet first, then a
/// spread of distinct hues so a busy chart's lines stay tellable apart.
pub const SERIES: [Color; 16] = [
    CYAN,
    GOLD,
    ORANGE,
    EMBER,
    Color::Rgb(0x3F, 0xB9, 0x50), // green
    Color::Rgb(0xC0, 0x7B, 0xFF), // violet
    Color::Rgb(0x2B, 0xC9, 0xB4), // teal
    Color::Rgb(0xFF, 0x7A, 0xB6), // pink
    Color::Rgb(0x5A, 0xA9, 0xFF), // sky
    Color::Rgb(0xB5, 0xD3, 0x3D), // lime
    Color::Rgb(0xE8, 0x6F, 0x4D), // coral
    Color::Rgb(0x8A, 0xD3, 0xFF), // ice
    Color::Rgb(0xD9, 0x57, 0xC7), // magenta
    Color::Rgb(0x6F, 0xE3, 0xA1), // mint
    Color::Rgb(0xF2, 0xC2, 0x4B), // amber
    Color::Rgb(0xA8, 0x90, 0xFF), // lavender
];

/// The accent style for emphasised, interactive text (bold cyan): table header emphasis, focus.
pub fn accent() -> Style {
    Style::default().fg(CYAN).add_modifier(Modifier::BOLD)
}

/// The brand style (bold gold): the app title and other identity moments.
pub fn brand() -> Style {
    Style::default().fg(GOLD).add_modifier(Modifier::BOLD)
}
