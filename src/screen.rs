use std::fmt::Display;

use image::RgbaImage;
use xcap::{Monitor, Window};

use crate::copy;
use crate::error::CommandError;
use crate::settings::fail;

#[derive(Debug, Clone)]
pub struct MonitorInfo {
    pub id: String,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
}

#[derive(Debug, Clone)]
pub struct WindowInfo {
    pub id: String,
    pub title: String,
    pub app_name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

pub fn list_monitors(locale: &str) -> Result<Vec<MonitorInfo>, CommandError> {
    let monitors = Monitor::all().map_err(|e| map_xcap(locale, e))?;
    let mut infos = Vec::new();
    for monitor in monitors {
        infos.push(read_monitor(locale, &monitor)?);
    }
    if infos.is_empty() {
        return Err(fail(locale, "NO_MONITOR"));
    }
    Ok(infos)
}

pub fn capture_monitor(locale: &str, monitor_id: Option<&str>) -> Result<(MonitorInfo, RgbaImage), CommandError> {
    let monitors = Monitor::all().map_err(|e| map_xcap(locale, e))?;
    let monitor = pick_monitor(locale, &monitors, monitor_id)?;
    let info = read_monitor(locale, monitor)?;
    let image = crate::veil::without_self(|| monitor.capture_image()).map_err(|e| map_xcap(locale, e))?;
    Ok((info, image))
}

pub fn list_windows(locale: &str) -> Result<Vec<WindowInfo>, CommandError> {
    let windows = Window::all().map_err(|e| map_xcap(locale, e))?;
    let mut infos = Vec::new();
    for window in windows {
        let Ok(info) = read_window(&window) else { continue };
        if !useful(&info) {
            continue;
        }
        infos.push(info);
    }
    infos.reverse();
    Ok(infos)
}

pub fn capture_window(locale: &str, window_id: &str) -> Result<(WindowInfo, RgbaImage), CommandError> {
    let windows = Window::all().map_err(|e| map_xcap(locale, e))?;
    let window = windows
        .iter()
        .find(|window| window.id().map(|id| id.to_string()).unwrap_or_default() == window_id)
        .ok_or_else(|| fail(locale, "WINDOW_NOT_FOUND"))?;
    if window.is_minimized().unwrap_or(false) {
        return Err(fail(locale, "WINDOW_MINIMIZED"));
    }
    let info = read_window(window).map_err(|e| map_xcap(locale, e))?;
    let image = crate::veil::without_self(|| window.capture_image()).map_err(|e| map_xcap(locale, e))?;
    Ok((info, image))
}

pub fn window_bounds(locale: &str, window_id: &str) -> Result<WindowInfo, CommandError> {
    let windows = Window::all().map_err(|e| map_xcap(locale, e))?;
    let window = windows
        .iter()
        .find(|window| window.id().map(|id| id.to_string()).unwrap_or_default() == window_id)
        .ok_or_else(|| fail(locale, "WINDOW_NOT_FOUND"))?;
    if window.is_minimized().unwrap_or(false) {
        return Err(fail(locale, "WINDOW_MINIMIZED"));
    }
    read_window(window).map_err(|e| map_xcap(locale, e))
}

fn pick_monitor<'a>(locale: &str, monitors: &'a [Monitor], id: Option<&str>) -> Result<&'a Monitor, CommandError> {
    if let Some(id) = id {
        if let Some(found) = monitors.iter().find(|m| m.id().map(|v| v.to_string()).unwrap_or_default() == id) {
            return Ok(found);
        }
    }
    monitors
        .iter()
        .find(|m| m.is_primary().unwrap_or(false))
        .or_else(|| monitors.first())
        .ok_or_else(|| fail(locale, "NO_MONITOR"))
}

fn read_monitor(locale: &str, monitor: &Monitor) -> Result<MonitorInfo, CommandError> {
    let name = monitor
        .friendly_name()
        .or_else(|_| monitor.name())
        .unwrap_or_else(|_| "Écran".into());
    Ok(MonitorInfo {
        id: monitor.id().map_err(|e| map_xcap(locale, e))?.to_string(),
        name,
        x: monitor.x().map_err(|e| map_xcap(locale, e))?,
        y: monitor.y().map_err(|e| map_xcap(locale, e))?,
        width: monitor.width().map_err(|e| map_xcap(locale, e))?,
        height: monitor.height().map_err(|e| map_xcap(locale, e))?,
        primary: monitor.is_primary().unwrap_or(false),
    })
}

fn read_window(window: &Window) -> Result<WindowInfo, xcap::XCapError> {
    Ok(WindowInfo {
        id: window.id()?.to_string(),
        title: window.title().unwrap_or_default(),
        app_name: window.app_name().unwrap_or_default(),
        x: window.x().unwrap_or(0),
        y: window.y().unwrap_or(0),
        width: window.width().unwrap_or(0),
        height: window.height().unwrap_or(0),
    })
}

fn useful(info: &WindowInfo) -> bool {
    let title = info.title.trim();
    if title.is_empty() || info.width < 80 || info.height < 60 {
        return false;
    }
    let lower = title.to_lowercase();
    let app = info.app_name.to_lowercase();
    if lower.contains("kapture") || app.contains("kapture") {
        return false;
    }
    if lower == "desktop" || lower.contains("program manager") || lower == "plasma" {
        return false;
    }
    true
}

pub fn map_xcap(locale: &str, err: impl Display) -> CommandError {
    let text = err.to_string();
    let lower = text.to_lowercase();
    if lower.contains("permission")
        || lower.contains("denied")
        || lower.contains("not authorized")
        || lower.contains("portal")
    {
        fail(locale, "PERMISSION")
    } else {
        CommandError::new(
            "CAPTURE_FAILED",
            format!("{} {text}", copy::text(locale, "CAPTURE_FAILED")),
        )
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn captures_the_primary_monitor_when_a_display_is_attached() {
        if std::env::var_os("DISPLAY").is_none() {
            return;
        }
        let (info, image) = capture_monitor("fr", None).expect("capture");
        assert!(info.width > 100 && info.height > 100);
        assert_eq!(image.width(), info.width);
        assert_eq!(image.height(), info.height);
    }
}
