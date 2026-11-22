/// Run `body` while Kapture's own windows are unmapped, so a still capture
/// does not include the application.
pub fn without_self<T>(body: impl FnOnce() -> T) -> T {
    let _guard = Conceal::hide();
    body()
}

pub fn is_self_label(value: &str) -> bool {
    value.to_lowercase().contains("kapture")
}

struct Conceal;

impl Conceal {
    fn hide() -> Option<Guard> {
        #[cfg(target_os = "linux")]
        {
            return linux::hide();
        }
        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    }
}

struct Guard {
    #[cfg(target_os = "linux")]
    inner: linux::Hidden,
}

impl Drop for Guard {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        linux::show(&self.inner);
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{is_self_label, Guard};
    use std::ffi::{c_char, c_int, c_ulong, c_void};
    use std::thread;
    use std::time::Duration;

    type Display = c_void;
    type XWindow = c_ulong;

    unsafe extern "C" {
        fn XOpenDisplay(name: *const c_char) -> *mut Display;
        fn XCloseDisplay(display: *mut Display) -> c_int;
        fn XUnmapWindow(display: *mut Display, window: XWindow) -> c_int;
        fn XMapWindow(display: *mut Display, window: XWindow) -> c_int;
        fn XRaiseWindow(display: *mut Display, window: XWindow) -> c_int;
        fn XFlush(display: *mut Display) -> c_int;
        fn XSync(display: *mut Display, discard: c_int) -> c_int;
    }

    #[link(name = "X11")]
    unsafe extern "C" {}

    pub struct Hidden {
        display: *mut Display,
        windows: Vec<XWindow>,
    }

    pub fn hide() -> Option<Guard> {
        unsafe {
            let display = XOpenDisplay(std::ptr::null());
            if display.is_null() {
                return None;
            }
            let windows = own_ids();
            if windows.is_empty() {
                XCloseDisplay(display);
                return None;
            }
            for window in &windows {
                XUnmapWindow(display, *window);
            }
            XFlush(display);
            XSync(display, 0);
            thread::sleep(Duration::from_millis(160));
            Some(Guard {
                inner: Hidden { display, windows },
            })
        }
    }

    fn own_ids() -> Vec<XWindow> {
        let mut ids = Vec::new();
        let pid = std::process::id().to_string();
        if let Ok(output) = std::process::Command::new("xdotool").args(["search", "--pid", &pid]).output() {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                if let Ok(id) = line.trim().parse::<XWindow>() {
                    ids.push(id);
                }
            }
        }
        if ids.is_empty() {
            if let Ok(windows) = xcap::Window::all() {
                for window in windows {
                    let title = window.title().unwrap_or_default();
                    let app = window.app_name().unwrap_or_default();
                    if is_self_label(&title) || is_self_label(&app) {
                        if let Ok(id) = window.id() {
                            ids.push(id as XWindow);
                        }
                    }
                }
            }
        }
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    pub fn show(hidden: &Hidden) {
        unsafe {
            for window in &hidden.windows {
                XMapWindow(hidden.display, *window);
                XRaiseWindow(hidden.display, *window);
            }
            XFlush(hidden.display);
            XSync(hidden.display, 0);
            XCloseDisplay(hidden.display);
        }
    }

}

#[cfg(test)]
mod tests {
    use super::is_self_label;

    #[test]
    fn only_kapture_windows_are_hidden() {
        assert!(is_self_label("Kapture"));
        assert!(is_self_label("Kapture Desktop"));
        assert!(!is_self_label("Firefox"));
        assert!(!is_self_label(""));
    }
}
