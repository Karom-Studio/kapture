#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostOs {
    Linux,
    Macos,
    Windows,
}

impl HostOs {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            HostOs::Macos
        } else if cfg!(target_os = "windows") {
            HostOs::Windows
        } else {
            HostOs::Linux
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            HostOs::Linux => "linux",
            HostOs::Macos => "macos",
            HostOs::Windows => "windows",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopSession {
    X11,
    Wayland,
    Macos,
    Windows,
}

impl DesktopSession {
    pub fn detect() -> Self {
        match HostOs::current() {
            HostOs::Macos => DesktopSession::Macos,
            HostOs::Windows => DesktopSession::Windows,
            HostOs::Linux => {
                let wayland = std::env::var_os("WAYLAND_DISPLAY")
                    .is_some_and(|v| !v.is_empty());
                let x11 = std::env::var_os("DISPLAY").is_some_and(|v| !v.is_empty());
                if wayland && !x11 {
                    DesktopSession::Wayland
                } else {
                    DesktopSession::X11
                }
            }
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            DesktopSession::X11 => "x11",
            DesktopSession::Wayland => "wayland",
            DesktopSession::Macos => "macos",
            DesktopSession::Windows => "windows",
        }
    }
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

pub fn screen_access_granted() -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        Some(unsafe { CGPreflightScreenCaptureAccess() })
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

pub fn request_screen_access() -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        Some(unsafe { CGRequestScreenCaptureAccess() })
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}
