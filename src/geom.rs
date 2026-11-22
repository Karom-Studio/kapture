/// Geometry helpers shared by screenshots, region selection and the encoder.
/// Video dimensions are forced to even numbers because yuv420p requires it.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PxRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

pub fn even_down(n: u32) -> u32 {
    let n = n.max(2);
    if n % 2 == 1 { n - 1 } else { n }
}

/// Fit `src` inside an optional maximum height. Never upscales.
pub fn fit_within(src_w: u32, src_h: u32, max_h: Option<u32>) -> (u32, u32) {
    let src_w = src_w.max(2);
    let src_h = src_h.max(2);
    let Some(max_h) = max_h else {
        return (even_down(src_w), even_down(src_h));
    };
    if max_h < 2 || src_h <= max_h {
        return (even_down(src_w), even_down(src_h));
    }
    let h = even_down(max_h);
    let w = even_down(((src_w as u64 * h as u64) / src_h as u64) as u32);
    (w, h)
}

/// Clamp a possibly-negative drag rectangle into a bitmap.
pub fn clamp_rect(x: i32, y: i32, w: i32, h: i32, bounds_w: u32, bounds_h: u32) -> Option<PxRect> {
    if bounds_w < 2 || bounds_h < 2 || w == 0 || h == 0 {
        return None;
    }
    let (x, w) = normalize(x, w);
    let (y, h) = normalize(y, h);
    if x >= bounds_w as i32 || y >= bounds_h as i32 {
        return None;
    }
    let x0 = x.max(0) as u32;
    let y0 = y.max(0) as u32;
    let x_trim = if x < 0 { (-x) as u32 } else { 0 };
    let y_trim = if y < 0 { (-y) as u32 } else { 0 };
    let width = (w as u32).saturating_sub(x_trim).min(bounds_w - x0);
    let height = (h as u32).saturating_sub(y_trim).min(bounds_h - y0);
    if width < 2 || height < 2 {
        return None;
    }
    Some(PxRect { x: x0, y: y0, width, height })
}

fn normalize(origin: i32, span: i32) -> (i32, i32) {
    if span < 0 {
        (origin + span, -span)
    } else {
        (origin, span)
    }
}

pub fn even_screen_rect(rect: ScreenRect) -> ScreenRect {
    ScreenRect {
        x: rect.x,
        y: rect.y,
        width: even_down(rect.width),
        height: even_down(rect.height),
    }
}

pub fn bubble_diameter(requested: u32, frame_h: u32) -> u32 {
    let cap = frame_h.saturating_sub(36).max(64);
    even_down(requested.min(cap).max(64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn even_dimensions_round_down() {
        assert_eq!(even_down(1920), 1920);
        assert_eq!(even_down(1919), 1918);
        assert_eq!(even_down(1), 2);
    }

    #[test]
    fn fit_never_upscales_and_caps_at_target() {
        assert_eq!(fit_within(1920, 1200, None), (1920, 1200));
        assert_eq!(fit_within(1920, 1200, Some(2160)), (1920, 1200));
        assert_eq!(fit_within(3840, 2160, Some(1080)), (1920, 1080));
        assert_eq!(fit_within(1920, 1200, Some(720)), (1152, 720));
        assert_eq!(fit_within(1919, 1079, Some(720)), (1280, 720));
    }

    #[test]
    fn clamp_handles_edges_and_negative_drag() {
        assert_eq!(
            clamp_rect(10, 12, 40, 30, 100, 80),
            Some(PxRect { x: 10, y: 12, width: 40, height: 30 })
        );
        assert_eq!(
            clamp_rect(-5, -4, 20, 18, 100, 80),
            Some(PxRect { x: 0, y: 0, width: 15, height: 14 })
        );
        assert_eq!(
            clamp_rect(90, 70, 40, 40, 100, 80),
            Some(PxRect { x: 90, y: 70, width: 10, height: 10 })
        );
        assert_eq!(
            clamp_rect(40, 20, -15, -10, 100, 80),
            Some(PxRect { x: 25, y: 10, width: 15, height: 10 })
        );
        assert!(clamp_rect(200, 10, 10, 10, 100, 80).is_none());
        assert!(clamp_rect(0, 0, 1, 10, 100, 80).is_none());
    }

    #[test]
    fn bubble_shrinks_on_short_frames() {
        assert_eq!(bubble_diameter(260, 1080), 260);
        assert_eq!(bubble_diameter(400, 200), 164);
    }
}
