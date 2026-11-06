//! Text without a browser engine. Glyphs are shaped and rasterized on demand into a
//! single atlas, with system fonts loaded only when a filename needs one.

use rustc_hash::FxHashMap;
use std::sync::Arc;
use swash::scale::{Render, ScaleContext, Source, StrikeWith, image::Content};
use swash::shape::ShapeContext;
use swash::shape::Direction;
use swash::text::{Codepoint, Script};
use swash::zeno::{Format, Vector};
use swash::{CacheKey, Charmap, FontRef, Setting};

pub const ATLAS_SIZE: u32 = 2048;
const PADDING: u32 = 1;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Face {
    /// Labels and interface text, regular weight.
    Ui,
    /// Labels and interface text, bold weight.
    UiBold,
    /// Large titles in the panel.
    Display,
    /// File contents in the text preview.
    Mono,
}

struct Font {
    data: Arc<Vec<u8>>,
    offset: u32,
    key: CacheKey,
}

impl Font {
    fn from_bytes(data: Arc<Vec<u8>>, index: usize) -> Option<Font> {
        let font = FontRef::from_index(&data, index)?;
        let (offset, key) = (font.offset, font.key);
        Some(Font { data, offset, key })
    }

    fn as_ref(&self) -> FontRef<'_> {
        FontRef { data: &self.data, offset: self.offset, key: self.key }
    }

    fn charmap(&self) -> Charmap<'_> {
        self.as_ref().charmap()
    }
}

#[derive(Clone, Copy)]
pub struct AtlasGlyph {
    pub uv: [u16; 4],
    pub left: i16,
    pub top: i16,
    pub width: u16,
    pub height: u16,
}

#[derive(Clone, Copy)]
pub struct PlacedGlyph {
    pub font: u8,
    pub id: u16,
    pub x: f32,
    pub y: f32,
}

pub struct Run {
    pub glyphs: Box<[PlacedGlyph]>,
    pub width: f32,
    last_used: u32,
}

/// A pending atlas upload.
pub struct Upload {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

#[derive(PartialEq, Eq, Hash, Clone, Copy)]
struct GlyphKey {
    font: u8,
    id: u16,
    size: u16,
    weight: u16,
    subpixel: u8,
}

#[derive(PartialEq, Eq, Hash, Clone, Copy)]
struct RunKey {
    hash: u64,
    len: u32,
    face: Face,
    size: u16,
}

pub struct Text {
    fonts: Vec<Font>,
    /// Candidate fallback files not yet opened.
    fallback_files: Vec<(&'static str, usize)>,
    missing_chars: rustc_hash::FxHashSet<char>,
    scale: ScaleContext,
    shape: ShapeContext,
    glyphs: FxHashMap<GlyphKey, Option<AtlasGlyph>>,
    runs: FxHashMap<RunKey, Run>,
    frame: u32,
    shelf_x: u32,
    shelf_y: u32,
    shelf_h: u32,
    pub uploads: Vec<Upload>,
    /// Set when the atlas was cleared and everything drawn this frame must be redone.
    pub atlas_reset: bool,
}

const FONT_UI: &[u8] = include_bytes!("../assets/fonts/Figtree.ttf");
const FONT_DISPLAY: &[u8] = include_bytes!("../assets/fonts/Bricolage.ttf");
const FONT_MONO: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono.ttf");

const IDX_UI: u8 = 0;
const IDX_DISPLAY: u8 = 1;
const IDX_MONO: u8 = 2;

fn face_font(face: Face) -> u8 {
    match face {
        Face::Ui | Face::UiBold => IDX_UI,
        Face::Display => IDX_DISPLAY,
        Face::Mono => IDX_MONO,
    }
}

fn face_weight(face: Face) -> f32 {
    match face {
        Face::Ui => 500.0,
        Face::UiBold => 700.0,
        Face::Display => 700.0,
        Face::Mono => 450.0,
    }
}

impl Text {
    pub fn new() -> Text {
        let mut fonts = Vec::new();
        for bytes in [FONT_UI, FONT_DISPLAY, FONT_MONO] {
            if let Some(font) = Font::from_bytes(Arc::new(bytes.to_vec()), 0) {
                fonts.push(font);
            }
        }
        assert_eq!(fonts.len(), 3, "bundled fonts must parse");
        Text {
            fonts,
            fallback_files: fallback_candidates(),
            missing_chars: Default::default(),
            scale: ScaleContext::new(),
            shape: ShapeContext::new(),
            glyphs: FxHashMap::default(),
            runs: FxHashMap::default(),
            frame: 0,
            shelf_x: 0,
            shelf_y: 0,
            shelf_h: 0,
            uploads: Vec::new(),
            atlas_reset: false,
        }
    }

    pub fn begin_frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        if self.frame % 600 == 0 && self.runs.len() > 4000 {
            let frame = self.frame;
            self.runs.retain(|_, run| frame.wrapping_sub(run.last_used) < 600);
        }
    }

    fn font_for(&mut self, ch: char, preferred: u8) -> u8 {
        if self.fonts[preferred as usize].charmap().map(ch) != 0 {
            return preferred;
        }
        for i in 0..self.fonts.len() {
            if i as u8 != IDX_MONO && self.fonts[i].charmap().map(ch) != 0 {
                return i as u8;
            }
        }
        if !self.missing_chars.contains(&ch) {
            // Open system fonts one by one, only as far as this character requires.
            while !self.fallback_files.is_empty() {
                let (path, index) = self.fallback_files.remove(0);
                if let Ok(bytes) = std::fs::read(path)
                    && let Some(font) = Font::from_bytes(Arc::new(bytes), index)
                    && self.fonts.len() < 250
                {
                    self.fonts.push(font);
                    let id = self.fonts.len() - 1;
                    if self.fonts[id].charmap().map(ch) != 0 {
                        return id as u8;
                    }
                }
            }
            self.missing_chars.insert(ch);
        }
        if self.fonts[IDX_MONO as usize].charmap().map(ch) != 0 { IDX_MONO } else { preferred }
    }

    /// Shapes text once and reuses the result, so labels cost almost nothing per frame.
    pub fn run(&mut self, text: &str, face: Face, size: f32) -> &Run {
        let size_key = (size * 4.0).round() as u16;
        let key = RunKey { hash: hash64(text), len: text.len() as u32, face, size: size_key };
        if !self.runs.contains_key(&key) {
            let run = self.shape_text(text, face, size);
            self.runs.insert(key, run);
        }
        let frame = self.frame;
        let run = self.runs.get_mut(&key).expect("just inserted");
        run.last_used = frame;
        run
    }

    fn shape_text(&mut self, text: &str, face: Face, size: f32) -> Run {
        let preferred = face_font(face);
        let weight = face_weight(face);
        let mut segments: Vec<(u8, Script, std::ops::Range<usize>)> = Vec::new();
        for (i, ch) in text.char_indices() {
            let font = self.font_for(ch, preferred);
            let script = ch.script();
            let script = if matches!(script, Script::Common | Script::Inherited | Script::Unknown) {
                segments.last().map_or(Script::Latin, |s| s.1)
            } else {
                script
            };
            match segments.last_mut() {
                Some(seg) if seg.0 == font && seg.1 == script => seg.2.end = i + ch.len_utf8(),
                _ => segments.push((font, script, i..i + ch.len_utf8())),
            }
        }

        // A name that starts in a right-to-left script reads from the right as a whole.
        let first_strong = text
            .chars()
            .map(|c| c.script())
            .find(|s| !matches!(s, Script::Common | Script::Inherited | Script::Unknown));
        if first_strong.is_some_and(is_rtl) {
            segments.reverse();
        }

        let mut glyphs = Vec::with_capacity(text.len());
        let mut pen = 0.0f32;
        let settings = [Setting { tag: swash::tag_from_bytes(b"wght"), value: weight }];
        for (font_idx, script, range) in segments {
            let font = &self.fonts[font_idx as usize];
            let mut shaper = self
                .shape
                .builder(font.as_ref())
                .script(script)
                .direction(if is_rtl(script) { Direction::RightToLeft } else { Direction::LeftToRight })
                .size(size)
                .variations(settings.iter().copied())
                .build();
            shaper.add_str(&text[range]);
            let mut clusters: Vec<Vec<(u16, f32, f32, f32)>> = Vec::new();
            shaper.shape_with(|cluster| {
                clusters.push(cluster.glyphs.iter().map(|g| (g.id, g.x, g.y, g.advance)).collect());
            });
            // Clusters arrive in reading order; a right-to-left run is laid out from its end.
            if is_rtl(script) {
                clusters.reverse();
            }
            for cluster in clusters {
                for (id, x, y, advance) in cluster {
                    glyphs.push(PlacedGlyph { font: font_idx, id, x: pen + x, y });
                    pen += advance;
                }
            }
        }
        Run { glyphs: glyphs.into_boxed_slice(), width: pen, last_used: self.frame }
    }

    pub fn width(&mut self, text: &str, face: Face, size: f32) -> f32 {
        self.run(text, face, size).width
    }

    /// Fetches a glyph's atlas slot, rasterizing it on first use.
    pub fn glyph(&mut self, font: u8, id: u16, face: Face, size: f32, subpixel: u8) -> Option<AtlasGlyph> {
        let weight = face_weight(face);
        let key = GlyphKey { font, id, size: (size * 4.0).round() as u16, weight: weight as u16, subpixel };
        if let Some(found) = self.glyphs.get(&key) {
            return *found;
        }
        let rendered = self.rasterize(font, id, size, weight, subpixel);
        self.glyphs.insert(key, rendered);
        rendered
    }

    fn rasterize(&mut self, font: u8, id: u16, size: f32, weight: f32, subpixel: u8) -> Option<AtlasGlyph> {
        let font_ref = self.fonts[font as usize].as_ref();
        let settings = [Setting { tag: swash::tag_from_bytes(b"wght"), value: weight }];
        let mut scaler = self
            .scale
            .builder(font_ref)
            .size(size)
            .hint(false)
            .variations(settings.iter().copied())
            .build();
        let offset = Vector::new(subpixel as f32 / 4.0, 0.0);
        let image = Render::new(&[Source::Outline, Source::Bitmap(StrikeWith::BestFit)])
            .format(Format::Alpha)
            .offset(offset)
            .render(&mut scaler, id)?;
        let (w, h) = (image.placement.width, image.placement.height);
        if w == 0 || h == 0 {
            return None;
        }
        // Colour bitmaps (emoji) are stored as their alpha so they read as shapes.
        let pixels: Vec<u8> = match image.content {
            Content::Mask => image.data,
            Content::Color | Content::SubpixelMask => image.data.chunks_exact(4).map(|p| p[3]).collect(),
        };
        let (x, y) = self.allocate(w, h)?;
        self.uploads.push(Upload { x, y, width: w, height: h, pixels });
        Some(AtlasGlyph {
            uv: [x as u16, y as u16, (x + w) as u16, (y + h) as u16],
            left: image.placement.left as i16,
            top: image.placement.top as i16,
            width: w as u16,
            height: h as u16,
        })
    }

    fn allocate(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        if w + PADDING > ATLAS_SIZE || h + PADDING > ATLAS_SIZE {
            return None;
        }
        if self.shelf_x + w + PADDING > ATLAS_SIZE {
            self.shelf_y += self.shelf_h + PADDING;
            self.shelf_x = 0;
            self.shelf_h = 0;
        }
        if self.shelf_y + h + PADDING > ATLAS_SIZE {
            // Full. Start over; anything still needed is rasterized again next frame.
            self.glyphs.clear();
            self.shelf_x = 0;
            self.shelf_y = 0;
            self.shelf_h = 0;
            self.atlas_reset = true;
        }
        let at = (self.shelf_x, self.shelf_y);
        self.shelf_x += w + PADDING;
        self.shelf_h = self.shelf_h.max(h);
        Some(at)
    }
}

fn is_rtl(script: Script) -> bool {
    matches!(script, Script::Arabic | Script::Hebrew | Script::Syriac | Script::Thaana | Script::Nko | Script::Adlam)
}

fn hash64(text: &str) -> u64 {
    use std::hash::Hasher;
    let mut hasher = rustc_hash::FxHasher::default();
    hasher.write(text.as_bytes());
    hasher.finish()
}

fn fallback_candidates() -> Vec<(&'static str, usize)> {
    #[cfg(windows)]
    {
        vec![
            ("C:\\Windows\\Fonts\\segoeui.ttf", 0),
            ("C:\\Windows\\Fonts\\YuGothM.ttc", 0),
            ("C:\\Windows\\Fonts\\msyh.ttc", 0),
            ("C:\\Windows\\Fonts\\malgun.ttf", 0),
            ("C:\\Windows\\Fonts\\Nirmala.ttc", 0),
            ("C:\\Windows\\Fonts\\seguisym.ttf", 0),
            ("C:\\Windows\\Fonts\\seguiemj.ttf", 0),
            ("C:\\Windows\\Fonts\\tahoma.ttf", 0),
            ("C:\\Windows\\Fonts\\arial.ttf", 0),
        ]
    }
    #[cfg(target_os = "macos")]
    {
        vec![
            ("/System/Library/Fonts/SFNS.ttf", 0),
            ("/System/Library/Fonts/Hiragino Sans GB.ttc", 0),
            ("/System/Library/Fonts/AppleSDGothicNeo.ttc", 0),
            ("/System/Library/Fonts/Supplemental/Arial Unicode.ttf", 0),
        ]
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        vec![
            ("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", 0),
            ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 0),
            ("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc", 0),
            ("/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf", 0),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_and_rasterizes_latin() {
        let mut text = Text::new();
        let width = text.width("Documents", Face::Ui, 14.0);
        assert!(width > 30.0 && width < 120.0, "{width}");
        let glyphs: Vec<PlacedGlyph> = text.run("A", Face::Ui, 14.0).glyphs.to_vec();
        let glyph = text.glyph(glyphs[0].font, glyphs[0].id, Face::Ui, 14.0, 0).unwrap();
        assert!(glyph.width > 0);
        assert_eq!(text.uploads.len(), 1);
    }

    #[test]
    fn bold_is_wider_than_regular() {
        let mut text = Text::new();
        let regular = text.width("Mountain range", Face::Ui, 20.0);
        let bold = text.width("Mountain range", Face::UiBold, 20.0);
        assert!(bold > regular, "{bold} vs {regular}");
    }

    #[test]
    fn runs_are_cached() {
        let mut text = Text::new();
        text.run("same", Face::Ui, 12.0);
        text.run("same", Face::Ui, 12.0);
        assert_eq!(text.runs.len(), 1);
    }
}
