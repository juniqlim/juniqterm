mod atlas;
mod renderer;

pub use atlas::GlyphAtlas;
pub use renderer::{GpuDrawer, TabBarInfo};

#[cfg(test)]
mod tests {
    use super::*;

    // --- GlyphAtlas: 래스터화 ---
    #[test]
    fn rasterize_ascii_produces_bitmap() {
        let mut atlas = GlyphAtlas::new(24.0, None);
        let glyph = atlas.get_or_insert('A');
        assert!(glyph.width > 0);
        assert!(glyph.height > 0);
        assert!(!glyph.bitmap.is_empty());
    }

    #[test]
    fn rasterize_space_has_zero_size() {
        let mut atlas = GlyphAtlas::new(24.0, None);
        let glyph = atlas.get_or_insert(' ');
        // space has no visible pixels
        assert_eq!(glyph.width, 0);
        assert_eq!(glyph.height, 0);
    }

    #[test]
    fn rasterize_cjk_produces_bitmap() {
        let mut atlas = GlyphAtlas::new(24.0, None);
        let glyph = atlas.get_or_insert('가');
        assert!(glyph.width > 0);
        assert!(glyph.height > 0);
    }

    #[test]
    fn rasterize_nerd_font_symbol() {
        let mut atlas = GlyphAtlas::new(24.0, None);
        let glyph = atlas.get_or_insert('☒'); // U+2612
        assert!(glyph.width > 0);
        assert!(glyph.height > 0);
    }

    // --- GlyphAtlas: 캐싱 ---
    #[test]
    fn second_lookup_returns_cached() {
        let mut atlas = GlyphAtlas::new(24.0, None);
        let first = atlas.get_or_insert('B');
        let first_bitmap = first.bitmap.clone();
        let second = atlas.get_or_insert('B');
        assert_eq!(first_bitmap, second.bitmap);
    }

    #[test]
    fn different_chars_have_different_bitmaps() {
        let mut atlas = GlyphAtlas::new(24.0, None);
        let a = atlas.get_or_insert('A');
        let a_bitmap = a.bitmap.clone();
        let b = atlas.get_or_insert('B');
        assert_ne!(a_bitmap, b.bitmap);
    }

    // --- GlyphAtlas: cell metrics ---
    #[test]
    fn cell_size_is_positive() {
        let atlas = GlyphAtlas::new(24.0, None);
        let (w, h) = atlas.cell_size();
        assert!(w > 0.0);
        assert!(h > 0.0);
    }
}
