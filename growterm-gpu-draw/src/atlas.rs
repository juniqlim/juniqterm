use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

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
    system_font_cache: HashMap<PathBuf, fontdue::Font>,
    char_to_font_path: HashMap<char, PathBuf>,
    size: f32,
    cache: HashMap<char, RasterizedGlyph>,
    cell_width: f32,
    cell_height: f32,
    ascent: f32,
}

impl GlyphAtlas {
    pub fn new(size: f32, font_path: Option<&str>) -> Self {
        let font = Arc::new(Self::load_font(size, font_path));
        let fallback_font = Arc::new(Self::load_fallback_font(size));
        Self::with_shared_fonts(size, font, fallback_font)
    }

    pub fn with_shared_fonts(size: f32, font: Arc<fontdue::Font>, fallback_font: Arc<fontdue::Font>) -> Self {
        let metrics = font.metrics('M', size);
        let line_metrics = font.horizontal_line_metrics(size);
        let (cell_height, ascent) = match line_metrics {
            Some(lm) => (lm.new_line_size, lm.ascent),
            None => (metrics.height as f32, metrics.height as f32 * 0.8),
        };

        Self {
            font,
            fallback_font,
            system_font_cache: HashMap::new(),
            char_to_font_path: HashMap::new(),
            size,
            cache: HashMap::new(),
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

    pub fn set_font(&mut self, font_path: Option<&str>, size: f32) {
        self.font = Arc::new(Self::load_font(size, font_path));
        self.size = size;
        self.cache.clear();
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
                        if let Ok(data) = std::fs::read(&path) {
                            let settings = fontdue::FontSettings {
                                scale: self.size,
                                ..Default::default()
                            };
                            if let Ok(font) = fontdue::Font::from_bytes(data, settings) {
                                if font.lookup_glyph_index(c) != 0 {
                                    self.char_to_font_path.insert(c, path_buf.clone());
                                    self.system_font_cache.insert(path_buf, font);
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
}
