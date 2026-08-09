use crate::utils::theme::Theme;
use ratatui::style::Color;

/// RGB triple stored as plain u8s for easy math.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub fn to_color(self) -> Color {
        Color::Rgb(self.r, self.g, self.b)
    }
}

/// Hue (degrees 0..360), saturation & lightness (0..1).
#[derive(Debug, Clone, Copy)]
struct Hsl {
    h: f32,
    s: f32,
    l: f32,
}

impl Hsl {
    fn new(h: f32, s: f32, l: f32) -> Self {
        Self {
            h: h.rem_euclid(360.0),
            s: s.clamp(0.0, 1.0),
            l: l.clamp(0.0, 1.0),
        }
    }
}

fn rgb_to_hsl(c: Rgb) -> Hsl {
    let r = f32::from(c.r) / 255.0;
    let g = f32::from(c.g) / 255.0;
    let b = f32::from(c.b) / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;

    if d.abs() < f32::EPSILON {
        return Hsl { h: 0.0, s: 0.0, l };
    }

    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if (max - r).abs() < f32::EPSILON {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if (max - g).abs() < f32::EPSILON {
        60.0 * (((b - r) / d) + 2.0)
    } else {
        60.0 * (((r - g) / d) + 4.0)
    };
    Hsl {
        h: h.rem_euclid(360.0),
        s,
        l,
    }
}

fn hsl_to_rgb(hsl: Hsl) -> Rgb {
    let c = (1.0 - (2.0 * hsl.l - 1.0).abs()) * hsl.s;
    let hp = hsl.h / 60.0;
    let x = c * (1.0 - (hp.rem_euclid(2.0) - 1.0).abs());
    let (r1, g1, b1) = match hp as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = hsl.l - c / 2.0;
    let to = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Rgb::new(to(r1), to(g1), to(b1))
}

fn hue_distance(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d)
}

fn vibrance(c: Rgb) -> f32 {
    let h = rgb_to_hsl(c);
    let l_penalty = 1.0 - (h.l - 0.55).abs() * 1.3;
    h.s * l_penalty.max(0.0)
}

const ACHROMATIC: f32 = 0.12;

fn for_dark_fg(c: Rgb) -> Rgb {
    let h = rgb_to_hsl(c);
    let s = if h.s < ACHROMATIC {
        h.s
    } else {
        h.s.clamp(0.45, 0.95)
    };
    hsl_to_rgb(Hsl::new(h.h, s, h.l.clamp(0.58, 0.76)))
}

/// Build a color from a base hue with explicit saturation & lightness.
fn tint(base_hue: f32, s: f32, l: f32) -> Rgb {
    hsl_to_rgb(Hsl::new(base_hue, s, l))
}


/// A box of pixels in RGB space, used by the median-cut algorithm.
#[derive(Clone)]
struct Box {
    pixels: Vec<Rgb>,
    min_r: u8,
    max_r: u8,
    min_g: u8,
    max_g: u8,
    min_b: u8,
    max_b: u8,
}

impl Box {
    fn new(mut pixels: Vec<Rgb>) -> Self {
        let (mut min_r, mut max_r) = (u8::MAX, 0u8);
        let (mut min_g, mut max_g) = (u8::MAX, 0u8);
        let (mut min_b, mut max_b) = (u8::MAX, 0u8);
        for p in &pixels {
            min_r = min_r.min(p.r);
            max_r = max_r.max(p.r);
            min_g = min_g.min(p.g);
            max_g = max_g.max(p.g);
            min_b = min_b.min(p.b);
            max_b = max_b.max(p.b);
        }
        let r_range = u32::from(max_r) - u32::from(min_r);
        let g_range = u32::from(max_g) - u32::from(min_g);
        let b_range = u32::from(max_b) - u32::from(min_b);
        if r_range >= g_range && r_range >= b_range {
            pixels.sort_by_key(|p| p.r);
        } else if g_range >= b_range {
            pixels.sort_by_key(|p| p.g);
        } else {
            pixels.sort_by_key(|p| p.b);
        }
        Self {
            pixels,
            min_r,
            max_r,
            min_g,
            max_g,
            min_b,
            max_b,
        }
    }

    fn volume(&self) -> u32 {
        (u32::from(self.max_r) - u32::from(self.min_r) + 1)
            * (u32::from(self.max_g) - u32::from(self.min_g) + 1)
            * (u32::from(self.max_b) - u32::from(self.min_b) + 1)
    }

    /// Split at the median along the pre-sorted axis. Returns two boxes.
    fn split(self) -> (Box, Box) {
        let mid = self.pixels.len() / 2;
        let left = self.pixels[..mid].to_vec();
        let right = self.pixels[mid..].to_vec();
        (Box::new(left), Box::new(right))
    }

    /// Average color of all pixels in the box.
    fn average(&self) -> Rgb {
        if self.pixels.is_empty() {
            return Rgb::new(0, 0, 0);
        }
        let n = self.pixels.len() as u32;
        let (sr, sg, sb) = self
            .pixels
            .iter()
            .fold((0u32, 0u32, 0u32), |(r, g, b), p| {
                (r + u32::from(p.r), g + u32::from(p.g), b + u32::from(p.b))
            });
        Rgb::new(
            (sr / n) as u8,
            (sg / n) as u8,
            (sb / n) as u8,
        )
    }
}

/// Extract `count` dominant colors from an image via median-cut quantization.
///
/// The image is first downsampled to a small thumbnail (32×32) so the
/// quantization runs on ~1k pixels regardless of source resolution.
pub fn extract_palette(img: &image::DynamicImage, count: usize) -> Vec<Rgb> {
    if count == 0 {
        return Vec::new();
    }

    // Downsample to 32×32 for speed. `thumbnail` preserves aspect ratio,
    // fitting within the box, so we get at most 1024 pixels.
    let thumb = img.thumbnail(32, 32);
    let rgba = thumb.to_rgba8();

    let mut pixels: Vec<Rgb> = rgba
        .pixels()
        .map(|p| Rgb::new(p.0[0], p.0[1], p.0[2]))
        .collect();

    // Deduplicate identical pixels to reduce work — a solid-color cover
    // would otherwise carry 1024 copies of the same value.
    pixels.sort_by(|a, b| {
        a.r.cmp(&b.r)
            .then(a.g.cmp(&b.g))
            .then(a.b.cmp(&b.b))
    });
    pixels.dedup();

    if pixels.is_empty() {
        return Vec::new();
    }

    let mut boxes = vec![Box::new(pixels)];

    while boxes.len() < count {
        // Pick the box with the largest volume to split — it contributes the
        // most color variance. Boxes with a single pixel can't be split.
        let mut max_idx = None;
        let mut max_vol = 0u32;
        for (i, b) in boxes.iter().enumerate() {
            if b.pixels.len() < 2 {
                continue;
            }
            let v = b.volume();
            if v > max_vol {
                max_vol = v;
                max_idx = Some(i);
            }
        }

        let Some(idx) = max_idx else { break };
        let b = boxes.remove(idx);
        let (left, right) = b.split();
        boxes.push(left);
        boxes.push(right);
    }

    let mut swatches: Vec<Rgb> = boxes.iter().map(|b| b.average()).collect();

    // Sort by vibrance descending — the most vivid colors first, so callers
    // can pick the top-N for accent roles.
    swatches.sort_by(|a, b| {
        vibrance(*b)
            .partial_cmp(&vibrance(*a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    swatches
}

pub fn derive_theme(swatches: &[Rgb], base: &Theme) -> Theme {
    if swatches.is_empty() {
        return base.clone();
    }

    let mut ranked: Vec<Rgb> = swatches.to_vec();
    ranked.sort_by(|a, b| {
        vibrance(*b)
            .partial_cmp(&vibrance(*a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let primary_raw = ranked[0];
    let primary = for_dark_fg(primary_raw);
    let primary_hsl = rgb_to_hsl(primary);

    let accent = if ranked.len() > 1 {
        let candidate = ranked
            .iter()
            .skip(1)
            .max_by(|a, b| {
                hue_distance(rgb_to_hsl(**a).h, primary_hsl.h)
                    .partial_cmp(&hue_distance(rgb_to_hsl(**b).h, primary_hsl.h))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .copied()
            .unwrap_or(primary_raw);
        for_dark_fg(candidate)
    } else {
        primary
    };

    let dominant_hue = primary_hsl.h;

    // Background layers — very dark, low-saturation tints of the dominant hue.
    // This gives the whole UI a subtle color cast from the album.
    let background = tint(dominant_hue, 0.15, 0.07);
    let background_panel = tint(dominant_hue, 0.18, 0.09);
    let background_element = tint(dominant_hue, 0.22, 0.12);

    // Borders — progressively lighter tints.
    let border_active = tint(dominant_hue, 0.30, 0.45);
    let border_inactive = tint(dominant_hue, 0.20, 0.35);
    let border_subtle = tint(dominant_hue, 0.15, 0.25);
    let border_dimmest = tint(dominant_hue, 0.10, 0.16);

    // Text — light, low-saturation, so it reads on the dark bg.
    let text_primary = tint(dominant_hue, 0.20, 0.85);
    let text_secondary = tint(dominant_hue, 0.15, 0.55);
    let status_bar = background_panel;
    let highlight_bg = background_element;

    // Semantic roles — derive from the swatches where possible.
    let success = for_dark_fg(
        ranked
            .iter()
            .find(|c| {
                let h = rgb_to_hsl(**c);
                (h.h - 90.0).abs() < 60.0 && h.s > 0.2
            })
            .copied()
            .unwrap_or(tint(dominant_hue, 0.5, 0.7)),
    );
    let error = for_dark_fg(
        ranked
            .iter()
            .find(|c| {
                let h = rgb_to_hsl(**c);
                (h.h - 0.0).abs() < 30.0 && h.s > 0.2
            })
            .copied()
            .unwrap_or(tint(0.0, 0.7, 0.65)),
    );
    let warning = for_dark_fg(
        ranked
            .iter()
            .find(|c| {
                let h = rgb_to_hsl(**c);
                (h.h - 40.0).abs() < 30.0 && h.s > 0.2
            })
            .copied()
            .unwrap_or(tint(40.0, 0.7, 0.65)),
    );
    let info = primary;

    let accent_color = accent;

    // Clone the base theme and overwrite only color fields.
    let mut t = base.clone();
    t.background = background.to_color();
    t.background_panel = background_panel.to_color();
    t.background_element = background_element.to_color();
    t.border_active = border_active.to_color();
    t.border_inactive = border_inactive.to_color();
    t.border_subtle = border_subtle.to_color();
    t.border_dimmest = border_dimmest.to_color();
    t.text_primary = text_primary.to_color();
    t.text_secondary = text_secondary.to_color();
    t.status_bar = status_bar.to_color();
    t.highlight_bg = highlight_bg.to_color();
    t.primary = primary.to_color();
    t.accent_color = accent_color.to_color();
    t.success = success.to_color();
    t.error = error.to_color();
    t.warning = warning.to_color();
    t.info = info.to_color();
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_image(r: u8, g: u8, b: u8) -> image::DynamicImage {
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(8, 8, image::Rgb([r, g, b])))
    }

    #[test]
    fn extract_palette_returns_correct_count() {
        let img = solid_image(100, 50, 200);
        let pal = extract_palette(&img, 3);
        assert!(!pal.is_empty());
        assert!(pal.len() <= 3);
    }

    #[test]
    fn extract_palette_solid_color() {
        let img = solid_image(100, 50, 200);
        let pal = extract_palette(&img, 1);
        assert_eq!(pal.len(), 1);
        // The single swatch should be close to the input color.
        assert!((i16::from(pal[0].r) - 100).abs() <= 5);
        assert!((i16::from(pal[0].g) - 50).abs() <= 5);
        assert!((i16::from(pal[0].b) - 200).abs() <= 5);
    }

    #[test]
    fn extract_palette_two_colors() {
        let mut img = image::RgbImage::new(8, 8);
        for y in 0..8 {
            for x in 0..8 {
                let color = if x < 4 {
                    image::Rgb([255, 0, 0])
                } else {
                    image::Rgb([0, 0, 255])
                };
                img.put_pixel(x, y, color);
            }
        }
        let dyn_img = image::DynamicImage::ImageRgb8(img);
        let pal = extract_palette(&dyn_img, 2);
        assert_eq!(pal.len(), 2);
        // Should contain a red-ish and a blue-ish color.
        let has_red = pal.iter().any(|c| c.r > 200 && c.b < 50);
        let has_blue = pal.iter().any(|c| c.b > 200 && c.r < 50);
        assert!(has_red, "palette should contain red: {:?}", pal);
        assert!(has_blue, "palette should contain blue: {:?}", pal);
    }

    #[test]
    fn derive_theme_has_valid_colors() {
        let base = Theme::default();
        let swatches = vec![Rgb::new(100, 150, 255), Rgb::new(255, 100, 100)];
        let t = derive_theme(&swatches, &base);
        // Primary should be bright enough for a dark terminal.
        let p = match t.primary {
            Color::Rgb(r, g, b) => (r, g, b),
            _ => (0, 0, 0),
        };
        // for_dark_fg clamps lightness to 0.58..0.76, so at least one channel
        // should be reasonably bright.
        assert!(p.0 > 100 || p.1 > 100 || p.2 > 100);
    }

    #[test]
    fn derive_theme_preserves_layout() {
        let base = Theme::default();
        let original_dir = base
            .layout_tree
            .direction;
        let original_children = base
            .layout_tree
            .children
            .as_ref()
            .map(|c| c.len());
        let swatches = vec![Rgb::new(100, 150, 255)];
        let t = derive_theme(&swatches, &base);
        // Layout fields should be untouched — only colors change.
        assert_eq!(t.layout_tree.direction, original_dir);
        assert_eq!(
            t.layout_tree.children.as_ref().map(|c| c.len()),
            original_children
        );
    }

    #[test]
    fn derive_theme_empty_swatches_returns_base() {
        let base = Theme::default();
        let t = derive_theme(&[], &base);
        assert_eq!(t.background, base.background);
    }

    #[test]
    fn hsl_roundtrip_is_stable() {
        for c in [
            Rgb::new(130, 170, 255),
            Rgb::new(40, 200, 90),
            Rgb::new(0, 0, 0),
            Rgb::new(255, 255, 255),
        ] {
            let back = hsl_to_rgb(rgb_to_hsl(c));
            assert!(
                (i16::from(back.r) - i16::from(c.r)).abs() <= 3,
                "{c:?} -> {back:?}"
            );
        }
    }

    #[test]
    fn hue_distance_wraps() {
        assert!((hue_distance(350.0, 10.0) - 20.0).abs() < 0.01);
        assert!((hue_distance(0.0, 180.0) - 180.0).abs() < 0.01);
    }

    #[test]
    fn for_dark_fg_lifts_dark_colors() {
        let dark = Rgb::new(20, 10, 40);
        let lifted = rgb_to_hsl(for_dark_fg(dark));
        assert!(lifted.l >= 0.57);
    }
}
