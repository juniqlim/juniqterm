use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use crate::renderer::GLYPH_LOG;

use core_foundation::array::CFArray;
use core_foundation::base::TCFType;
use core_foundation::string::CFString;
use core_text::font as ct_font;
use core_text::font::CTFontRef;

pub struct RasterizedGlyph {
    pub width: u32,
    pub height: u32,
    pub bitmap: Vec<u8>,
    pub offset_x: f32,
    pub offset_y: f32,
}

pub struct GlyphAtlas {
    font: Arc<fontdue::Font>,
    fallback_font: Arc<fontdue::Font>,
    bold_font: Arc<fontdue::Font>,
    bold_fallback_font: Arc<fontdue::Font>,
    system_font_cache: HashMap<PathBuf, fontdue::Font>,
    char_to_font_path: HashMap<char, PathBuf>,
    size: f32,
    cache: HashMap<char, RasterizedGlyph>,
    bold_cache: HashMap<char, RasterizedGlyph>,
    cell_width: f32,
    cell_height: f32,
    ascent: f32,
}

impl GlyphAtlas {
    pub fn new(size: f32, font_path: Option<&str>) -> Self {
        let font = Arc::new(Self::load_font(size, font_path));
        let fallback_font = Arc::new(Self::load_fallback_font(size));
        let bold_font = Arc::new(Self::load_builtin_bold_font(size));
        let bold_fallback_font = Arc::new(Self::load_fallback_bold_font(size));
        Self::with_shared_fonts(size, font, fallback_font, bold_font, bold_fallback_font)
    }

    pub fn with_shared_fonts(size: f32, font: Arc<fontdue::Font>, fallback_font: Arc<fontdue::Font>, bold_font: Arc<fontdue::Font>, bold_fallback_font: Arc<fontdue::Font>) -> Self {
        let metrics = font.metrics('M', size);
        let line_metrics = font.horizontal_line_metrics(size);
        let (cell_height, ascent) = match line_metrics {
            Some(lm) => (lm.new_line_size, lm.ascent),
            None => (metrics.height as f32, metrics.height as f32 * 0.8),
        };

        Self {
            font,
            fallback_font,
            bold_font,
            bold_fallback_font,
            system_font_cache: HashMap::new(),
            char_to_font_path: HashMap::new(),
            size,
            cache: HashMap::new(),
            bold_cache: HashMap::new(),
            cell_width: metrics.advance_width.ceil(),
            cell_height: cell_height.ceil(),
            ascent,
        }
    }

    pub fn load_font(size: f32, font_path: Option<&str>) -> fontdue::Font {
        if let Some(path) = font_path {
            if let Ok(data) = std::fs::read(path) {
                let settings = fontdue::FontSettings {
                    scale: size,
                    ..Default::default()
                };
                return fontdue::Font::from_bytes(data, settings).unwrap_or_else(|_| {
                    Self::load_builtin_font(size)
                });
            }
        }
        Self::load_builtin_font(size)
    }

    pub fn load_fallback_font(size: f32) -> fontdue::Font {
        let fallback_data = include_bytes!("../fonts/D2Coding.ttc");
        let fallback_settings = fontdue::FontSettings {
            scale: size,
            collection_index: 0,
            ..Default::default()
        };
        fontdue::Font::from_bytes(fallback_data as &[u8], fallback_settings)
            .expect("failed to load D2Coding fallback font")
    }

    pub fn load_builtin_font(size: f32) -> fontdue::Font {
        let font_data = include_bytes!("../fonts/FiraCodeNerdFontMono-Retina.ttf");
        let settings = fontdue::FontSettings {
            scale: size,
            ..Default::default()
        };
        fontdue::Font::from_bytes(font_data as &[u8], settings)
            .expect("failed to load Fira Code Nerd Font")
    }

    pub fn load_builtin_bold_font(size: f32) -> fontdue::Font {
        let font_data = include_bytes!("../fonts/FiraCodeNerdFontMono-Bold.ttf");
        let settings = fontdue::FontSettings {
            scale: size,
            ..Default::default()
        };
        fontdue::Font::from_bytes(font_data as &[u8], settings)
            .expect("failed to load Fira Code Nerd Font Bold")
    }

    pub fn load_fallback_bold_font(size: f32) -> fontdue::Font {
        let fallback_data = include_bytes!("../fonts/D2CodingBold.ttf");
        let settings = fontdue::FontSettings {
            scale: size,
            ..Default::default()
        };
        fontdue::Font::from_bytes(fallback_data as &[u8], settings)
            .expect("failed to load D2Coding Bold fallback font")
    }

    pub fn set_font(&mut self, font_path: Option<&str>, size: f32) {
        self.font = Arc::new(Self::load_font(size, font_path));
        self.size = size;
        self.cache.clear();
        self.bold_cache.clear();
        self.system_font_cache.clear();
        self.char_to_font_path.clear();
        let metrics = self.font.metrics('M', size);
        let line_metrics = self.font.horizontal_line_metrics(size);
        match line_metrics {
            Some(lm) => { self.cell_height = lm.new_line_size.ceil(); self.ascent = lm.ascent; }
            None => { self.cell_height = (metrics.height as f32).ceil(); self.ascent = metrics.height as f32 * 0.8; }
        }
        self.cell_width = metrics.advance_width.ceil();
    }

    pub fn set_size(&mut self, size: f32) {
        self.size = size;
        self.cache.clear();
        self.bold_cache.clear();
        self.system_font_cache.clear();
        self.char_to_font_path.clear();

        let metrics = self.font.metrics('M', size);
        let line_metrics = self.font.horizontal_line_metrics(size);
        match line_metrics {
            Some(lm) => { self.cell_height = lm.new_line_size.ceil(); self.ascent = lm.ascent; }
            None => { self.cell_height = (metrics.height as f32).ceil(); self.ascent = metrics.height as f32 * 0.8; }
        }
        self.cell_width = metrics.advance_width.ceil();
    }

    pub fn cell_size(&self) -> (f32, f32) {
        (self.cell_width, self.cell_height)
    }

    pub fn ascent(&self) -> f32 {
        self.ascent
    }

    fn find_system_font(&mut self, c: char) -> bool {
        if self.char_to_font_path.contains_key(&c) {
            return true;
        }

        // Check if any already-cached font has this glyph
        for (path, font) in &self.system_font_cache {
            if font.lookup_glyph_index(c) != 0 {
                self.char_to_font_path.insert(c, path.clone());
                if let Ok(mut guard) = GLYPH_LOG.lock() {
                    if let Some(f) = guard.as_mut() {
                        let _ = writeln!(f, "[font-cache] hit '{}' (U+{:04X}) in {:?}", c, c as u32, path);
                    }
                }
                return true;
            }
        }

        let base = ct_font::new_from_name("Helvetica", self.size as f64)
            .expect("failed to create CT font");
        let langs: CFArray<CFString> = CFArray::from_CFTypes(&[]);
        let cascade = ct_font::cascade_list_for_languages(&base, &langs);

        let mut utf16_buf = [0u16; 2];
        let utf16 = c.encode_utf16(&mut utf16_buf);
        let mut glyph_buf = [0u16; 2];

        for i in 0..cascade.len() {
            let descriptor = cascade.get(i).unwrap();
            let candidate = ct_font::new_from_descriptor(&descriptor, self.size as f64);

            let found = unsafe {
                extern "C" {
                    fn CTFontGetGlyphsForCharacters(
                        font: CTFontRef,
                        characters: *const u16,
                        glyphs: *mut u16,
                        count: isize,
                    ) -> bool;
                }
                CTFontGetGlyphsForCharacters(
                    candidate.as_concrete_TypeRef(),
                    utf16.as_ptr(),
                    glyph_buf.as_mut_ptr(),
                    utf16.len() as isize,
                )
            };

            if found && glyph_buf[0] != 0 {
                if let Some(url) = candidate.url() {
                    if let Some(path) = url.to_path() {
                        let path_buf = path.to_path_buf();
                        // Reuse already-loaded font for this path
                        if let Some(font) = self.system_font_cache.get(&path_buf) {
                            if font.lookup_glyph_index(c) != 0 {
                                self.char_to_font_path.insert(c, path_buf);
                                return true;
                            }
                            continue;
                        }
                        let read_start = std::time::Instant::now();
                        if let Ok(data) = std::fs::read(&path) {
                            let settings = fontdue::FontSettings {
                                scale: self.size,
                                ..Default::default()
                            };
                            if let Ok(font) = fontdue::Font::from_bytes(data, settings) {
                                let has_glyph = font.lookup_glyph_index(c) != 0;
                                if has_glyph {
                                    self.char_to_font_path.insert(c, path_buf.clone());
                                }
                                // Cache font regardless of whether it has this glyph,
                                // to avoid re-reading the same font file from disk.
                                if let Ok(mut guard) = GLYPH_LOG.lock() {
                                    if let Some(f) = guard.as_mut() {
                                        let _ = writeln!(f, "[font-disk] read+parse {:?} for '{}' (U+{:04X}) glyph={} {:.1}ms",
                                            path_buf, c, c as u32, has_glyph, read_start.elapsed().as_secs_f64() * 1000.0);
                                    }
                                }
                                self.system_font_cache.insert(path_buf, font);
                                if has_glyph {
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
        }
        false
    }

    pub fn get_or_insert(&mut self, c: char) -> &RasterizedGlyph {
        if !self.cache.contains_key(&c) {
            // find_system_font borrows &mut self, so call it before taking &self refs
            let system_font_path = if self.font.lookup_glyph_index(c) != 0 || self.fallback_font.lookup_glyph_index(c) != 0 {
                None
            } else if self.find_system_font(c) {
                Some(self.char_to_font_path.get(&c).unwrap().clone())
            } else {
                None
            };

            let font: &fontdue::Font = if self.font.lookup_glyph_index(c) != 0 {
                &self.font
            } else if self.fallback_font.lookup_glyph_index(c) != 0 {
                &self.fallback_font
            } else if let Some(ref path) = system_font_path {
                self.system_font_cache.get(path).unwrap()
            } else {
                &self.font
            };

            let (metrics, bitmap) = font.rasterize(c, self.size);
            self.cache.insert(c, RasterizedGlyph {
                width: metrics.width as u32,
                height: metrics.height as u32,
                bitmap,
                offset_x: metrics.xmin as f32,
                offset_y: metrics.ymin as f32,
            });
        }
        self.cache.get(&c).unwrap()
    }

    pub fn get_or_insert_bold(&mut self, c: char) -> &RasterizedGlyph {
        if !self.bold_cache.contains_key(&c) {
            let font: &fontdue::Font = if self.bold_font.lookup_glyph_index(c) != 0 {
                &self.bold_font
            } else if self.bold_fallback_font.lookup_glyph_index(c) != 0 {
                &self.bold_fallback_font
            } else if self.font.lookup_glyph_index(c) != 0 {
                // Fallback to normal font if no bold variant has this glyph
                &self.font
            } else if self.fallback_font.lookup_glyph_index(c) != 0 {
                &self.fallback_font
            } else {
                &self.font
            };

            let (metrics, bitmap) = font.rasterize(c, self.size);
            self.bold_cache.insert(c, RasterizedGlyph {
                width: metrics.width as u32,
                height: metrics.height as u32,
                bitmap,
                offset_x: metrics.xmin as f32,
                offset_y: metrics.ymin as f32,
            });
        }
        self.bold_cache.get(&c).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bold_glyph_differs_from_normal() {
        let size = 16.0;
        let normal_font = GlyphAtlas::load_builtin_font(size);
        let bold_font = GlyphAtlas::load_builtin_bold_font(size);

        let (_, normal_bitmap) = normal_font.rasterize('A', size);
        let (_, bold_bitmap) = bold_font.rasterize('A', size);

        assert_ne!(normal_bitmap, bold_bitmap, "Bold glyph should differ from normal");
    }

    #[test]
    fn bold_fallback_glyph_differs_from_normal() {
        let size = 16.0;
        let normal_font = GlyphAtlas::load_fallback_font(size);
        let bold_font = GlyphAtlas::load_fallback_bold_font(size);

        let (_, normal_bitmap) = normal_font.rasterize('가', size);
        let (_, bold_bitmap) = bold_font.rasterize('가', size);

        assert_ne!(normal_bitmap, bold_bitmap, "Bold Korean glyph should differ from normal");
    }

    #[test]
    fn get_or_insert_bold_returns_different_glyph() {
        let size = 16.0;
        let mut atlas = GlyphAtlas::new(size, None);

        let normal = atlas.get_or_insert('A');
        let normal_bitmap = normal.bitmap.clone();

        let bold = atlas.get_or_insert_bold('A');
        let bold_bitmap = bold.bitmap.clone();

        assert_ne!(normal_bitmap, bold_bitmap, "Bold cached glyph should differ from normal");
    }
}
