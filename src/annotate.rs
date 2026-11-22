use std::path::PathBuf;

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use image::{Rgba, RgbaImage};

#[derive(Clone, Debug)]
pub enum Mark {
    Stroke(Vec<(i32, i32)>),
    Arrow { a: (i32, i32), b: (i32, i32) },
    Rect { a: (i32, i32), b: (i32, i32) },
    Text { at: (i32, i32), text: String },
    Icon { at: (i32, i32), kind: u8 },
}

const INK: Rgba<u8> = Rgba([255, 90, 54, 255]);
const HALO: Rgba<u8> = Rgba([20, 16, 14, 255]);

pub fn paint(image: &mut RgbaImage, marks: &[Mark]) {
    for mark in marks {
        match mark {
            Mark::Stroke(points) => {
                for pair in points.windows(2) {
                    thick_line(image, pair[0], pair[1], 4, INK);
                }
                if let Some(point) = points.first() {
                    disk(image, *point, 4, INK);
                }
            }
            Mark::Arrow { a, b } => {
                thick_line(image, *a, *b, 4, INK);
                arrow_head(image, *a, *b);
            }
            Mark::Rect { a, b } => rect_outline(image, *a, *b, 3, INK),
            Mark::Text { at, text } => draw_text(image, *at, text),
            Mark::Icon { at, kind } => badge(image, *at, *kind),
        }
    }
}

pub fn font_path() -> Option<PathBuf> {
    [
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
        "/Library/Fonts/Arial.ttf",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "C:/Windows/Fonts/arial.ttf",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|path| path.is_file())
}

fn draw_text(image: &mut RgbaImage, at: (i32, i32), text: &str) {
    let Some(path) = font_path() else { return };
    let Ok(bytes) = std::fs::read(path) else { return };
    let Ok(font) = FontRef::try_from_slice(&bytes) else { return };
    let scale = PxScale::from(42.0);
    let font = font.as_scaled(scale);
    let mut cursor = at.0 as f32;
    let baseline = at.1 as f32 + font.ascent();
    for ch in text.chars() {
        let glyph = font.scaled_glyph(ch);
        let advance = font.h_advance(glyph.id);
        if let Some(outlined) = font.outline_glyph(glyph) {
            outlined.draw(|x, y, coverage| {
                if coverage < 0.15 {
                    return;
                }
                let px = cursor as i32 + outlined.px_bounds().min.x as i32 + x as i32;
                let py = baseline as i32 + outlined.px_bounds().min.y as i32 + y as i32;
                for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    put(image, px + dx, py + dy, HALO);
                }
                put(image, px, py, Rgba([255, 255, 255, 255]));
            });
        }
        cursor += advance;
    }
}

fn badge(image: &mut RgbaImage, at: (i32, i32), kind: u8) {
    disk(image, at, 26, HALO);
    disk(image, at, 22, Rgba([255, 255, 255, 255]));
    ring(image, at, 20, INK);
    let (x, y) = at;
    match kind {
        1 => {
            thick_line(image, (x - 9, y - 9), (x + 9, y + 9), 3, INK);
            thick_line(image, (x + 9, y - 9), (x - 9, y + 9), 3, INK);
        }
        2 => star(image, at, 14),
        _ => {
            thick_line(image, (x - 9, y + 1), (x - 2, y + 8), 3, INK);
            thick_line(image, (x - 2, y + 8), (x + 10, y - 8), 3, INK);
        }
    }
}

fn ring(image: &mut RgbaImage, center: (i32, i32), radius: i32, color: Rgba<u8>) {
    let mut previous = None;
    for step in 0..=48 {
        let angle = step as f32 / 48.0 * std::f32::consts::TAU;
        let point = (
            center.0 + (angle.cos() * radius as f32).round() as i32,
            center.1 + (angle.sin() * radius as f32).round() as i32,
        );
        if let Some(start) = previous {
            thick_line(image, start, point, 2, color);
        }
        previous = Some(point);
    }
}

fn star(image: &mut RgbaImage, center: (i32, i32), radius: i32) {
    let mut points = [(0, 0); 5];
    for index in 0..5 {
        let angle = -std::f32::consts::FRAC_PI_2 + index as f32 * std::f32::consts::TAU / 5.0;
        points[index] = (
            center.0 + (angle.cos() * radius as f32).round() as i32,
            center.1 + (angle.sin() * radius as f32).round() as i32,
        );
    }
    for index in 0..5 {
        thick_line(image, points[index], points[(index + 2) % 5], 2, INK);
    }
}

fn arrow_head(image: &mut RgbaImage, a: (i32, i32), b: (i32, i32)) {
    let dx = (b.0 - a.0) as f32;
    let dy = (b.1 - a.1) as f32;
    let len = (dx * dx + dy * dy).sqrt().max(1.0);
    let ux = dx / len;
    let uy = dy / len;
    let left = (b.0 as f32 - ux * 28.0 - uy * 14.0, b.1 as f32 - uy * 28.0 + ux * 14.0);
    let right = (b.0 as f32 - ux * 28.0 + uy * 14.0, b.1 as f32 - uy * 28.0 - ux * 14.0);
    thick_line(image, b, (left.0 as i32, left.1 as i32), 4, INK);
    thick_line(image, b, (right.0 as i32, right.1 as i32), 4, INK);
}

fn rect_outline(image: &mut RgbaImage, a: (i32, i32), b: (i32, i32), width: i32, color: Rgba<u8>) {
    let x0 = a.0.min(b.0);
    let x1 = a.0.max(b.0);
    let y0 = a.1.min(b.1);
    let y1 = a.1.max(b.1);
    thick_line(image, (x0, y0), (x1, y0), width, color);
    thick_line(image, (x1, y0), (x1, y1), width, color);
    thick_line(image, (x1, y1), (x0, y1), width, color);
    thick_line(image, (x0, y1), (x0, y0), width, color);
}

fn thick_line(image: &mut RgbaImage, a: (i32, i32), b: (i32, i32), radius: i32, color: Rgba<u8>) {
    let dx = (b.0 - a.0).abs();
    let dy = (b.1 - a.1).abs();
    let steps = dx.max(dy).max(1);
    for step in 0..=steps {
        let x = a.0 + (b.0 - a.0) * step / steps;
        let y = a.1 + (b.1 - a.1) * step / steps;
        disk(image, (x, y), radius, color);
    }
}

fn disk(image: &mut RgbaImage, center: (i32, i32), radius: i32, color: Rgba<u8>) {
    for y in -radius..=radius {
        for x in -radius..=radius {
            if x * x + y * y <= radius * radius {
                put(image, center.0 + x, center.1 + y, color);
            }
        }
    }
}

fn put(image: &mut RgbaImage, x: i32, y: i32, color: Rgba<u8>) {
    if x < 0 || y < 0 {
        return;
    }
    let (x, y) = (x as u32, y as u32);
    if x < image.width() && y < image.height() {
        image.put_pixel(x, y, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stroke_changes_pixels() {
        let mut image = RgbaImage::from_pixel(40, 40, Rgba([255, 255, 255, 255]));
        paint(&mut image, &[Mark::Stroke(vec![(4, 20), (36, 20)])]);
        assert_ne!(image.get_pixel(20, 20).0, [255, 255, 255, 255]);
    }

    #[test]
    fn icons_leave_a_mark() {
        let mut image = RgbaImage::from_pixel(80, 80, Rgba([255, 255, 255, 255]));
        paint(&mut image, &[Mark::Icon { at: (40, 40), kind: 0 }]);
        assert_ne!(image.get_pixel(40, 40).0, [255, 255, 255, 255]);
    }

    #[test]
    fn text_uses_the_system_font_when_present() {
        let mut image = RgbaImage::from_pixel(180, 80, Rgba([0, 0, 0, 255]));
        paint(&mut image, &[Mark::Text { at: (8, 8), text: "Kapture".into() }]);
        if font_path().is_some() {
            assert!(image.pixels().any(|pixel| pixel.0[0] > 200));
        }
    }
}
