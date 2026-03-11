//! UI components and rendering.

pub mod footer;
pub mod header;
pub mod layout;
pub mod overlays;
pub mod preview;
pub mod results;
pub mod search_input;
pub mod theme;

pub use theme::*;
use unicode_width::UnicodeWidthStr;

pub fn display_width_up_to(text: &str, byte_index: usize) -> usize {
    let mut clamped = byte_index.min(text.len());
    while clamped > 0 && !text.is_char_boundary(clamped) {
        clamped -= 1;
    }
    UnicodeWidthStr::width(&text[..clamped])
}

#[cfg(test)]
mod tests {
    use super::display_width_up_to;

    #[test]
    fn display_width_up_to_uses_cell_width_not_byte_length() {
        assert_eq!(display_width_up_to("éa", "é".len()), 1);
        assert_eq!(display_width_up_to("éa", "éa".len()), 2);
    }

    #[test]
    fn display_width_up_to_clamps_to_char_boundary() {
        assert_eq!(display_width_up_to("éa", 1), 0);
        assert_eq!(display_width_up_to("éa", 99), 2);
    }
}
