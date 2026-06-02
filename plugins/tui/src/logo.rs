//! The Alumet logo, shown in the "about" overlay (`?`).
//!
//! It is pre-rendered from `assets/logo.png` to a small RGBA grid (see `assets/logo.rgba`) and drawn
//! with half-block characters: each terminal cell stacks two vertical pixels — the top one as the
//! glyph's foreground, the bottom one as its background. Pre-rendering keeps the logo as the source
//! of truth without pulling in a runtime image-decoding dependency.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::theme;

/// Width, in cells, of the [`tag`] brand chip — `" Alumet "`, all single-width glyphs.
pub const TAG_WIDTH: u16 = 8;

/// A compact one-line brand chip — a gold flame tip and the wordmark in the logo's flame gradient
/// (gold → orange → ember) — pinned to the tab bar so every view reads as Alumet at a glance,
/// without the full logo's footprint. Think of it as the app's favicon.
pub fn tag() -> Line<'static> {
    let flame = |c: Color| Style::default().fg(c).add_modifier(Modifier::BOLD);
    Line::from(vec![
        Span::raw(" "),
        Span::styled("Al", flame(theme::GOLD)),
        Span::styled("um", flame(theme::ORANGE)),
        Span::styled("et", flame(theme::EMBER)),
        Span::raw(" "),
    ])
}

/// Raw RGBA pixels of the downscaled logo, row-major, 4 bytes per pixel.
const LOGO_RGBA: &[u8] = include_bytes!("../assets/logo.rgba");
const LOGO_W: usize = 21;
const LOGO_H: usize = 40;
/// Pixels with alpha below this are treated as transparent (left as the terminal background).
const ALPHA_THRESHOLD: u8 = 128;

/// Rendered logo width, in terminal cells (one cell per pixel column).
pub const WIDTH: u16 = LOGO_W as u16;
/// Rendered logo height, in terminal cells (two pixel rows per cell).
pub const HEIGHT: u16 = (LOGO_H / 2) as u16;

/// The logo as half-block lines, ready to drop into a `Paragraph`.
pub fn lines() -> Vec<Line<'static>> {
    (0..LOGO_H / 2).map(|cell_y| Line::from(row_spans(cell_y))).collect()
}

/// Builds the half-block spans for one cell-row, pairing pixel rows `2·cell_y` (top) and `+1` (bottom).
fn row_spans(cell_y: usize) -> Vec<Span<'static>> {
    (0..LOGO_W)
        .map(|x| {
            let top = pixel(x, cell_y * 2);
            let bottom = pixel(x, cell_y * 2 + 1);
            match (top, bottom) {
                // Nothing to draw: leave the terminal background showing.
                (None, None) => Span::raw(" "),
                // Upper half-block: foreground paints the top pixel, background the bottom.
                (Some(t), bottom) => {
                    Span::styled("\u{2580}", Style::default().fg(t).bg(bottom.unwrap_or(Color::Reset)))
                }
                // Only the bottom pixel is opaque: a lower half-block leaves the top transparent.
                (None, Some(b)) => Span::styled("\u{2584}", Style::default().fg(b)),
            }
        })
        .collect()
}

/// The color of pixel `(x, y)`, or `None` if it is transparent.
fn pixel(x: usize, y: usize) -> Option<Color> {
    let i = (y * LOGO_W + x) * 4;
    let (r, g, b, a) = (LOGO_RGBA[i], LOGO_RGBA[i + 1], LOGO_RGBA[i + 2], LOGO_RGBA[i + 3]);
    (a >= ALPHA_THRESHOLD).then_some(Color::Rgb(r, g, b))
}
