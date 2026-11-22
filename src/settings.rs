use std::str::FromStr;

use global_hotkey::hotkey::HotKey;
use serde::{Deserialize, Serialize};

use crate::copy;
use crate::error::{Cmd, CommandError};
use crate::paths::default_output_dir;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum VideoHeight {
    #[default]
    Native,
    #[serde(rename = "720")]
    H720,
    #[serde(rename = "1080")]
    H1080,
    #[serde(rename = "1440")]
    H1440,
    #[serde(rename = "2160")]
    H2160,
}

impl VideoHeight {
    /// Maximum output height. Native still caps at 4K and never upscales.
    pub fn max_height(self) -> u32 {
        match self {
            VideoHeight::Native | VideoHeight::H2160 => 2160,
            VideoHeight::H1440 => 1440,
            VideoHeight::H1080 => 1080,
            VideoHeight::H720 => 720,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum Quality {
    High,
    #[default]
    Balanced,
    Compact,
}

impl Quality {
    pub fn crf(self) -> u32 {
        match self {
            Quality::High => 18,
            Quality::Balanced => 23,
            Quality::Compact => 28,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum WebcamSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl WebcamSize {
    pub fn px(self) -> u32 {
        match self {
            WebcamSize::Small => 180,
            WebcamSize::Medium => 260,
            WebcamSize::Large => 340,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    #[default]
    BottomRight,
}

pub struct PipPreset {
    pub key: &'static str,
    pub enabled: bool,
    pub size: WebcamSize,
    pub corner: Corner,
}

pub fn pip_presets() -> &'static [PipPreset] {
    &[
        PipPreset { key: "off", enabled: false, size: WebcamSize::Medium, corner: Corner::BottomRight },
        PipPreset { key: "s-br", enabled: true, size: WebcamSize::Small, corner: Corner::BottomRight },
        PipPreset { key: "m-br", enabled: true, size: WebcamSize::Medium, corner: Corner::BottomRight },
        PipPreset { key: "l-br", enabled: true, size: WebcamSize::Large, corner: Corner::BottomRight },
        PipPreset { key: "s-tl", enabled: true, size: WebcamSize::Small, corner: Corner::TopLeft },
        PipPreset { key: "m-tl", enabled: true, size: WebcamSize::Medium, corner: Corner::TopLeft },
        PipPreset { key: "l-bl", enabled: true, size: WebcamSize::Large, corner: Corner::BottomLeft },
    ]
}

impl Settings {
    pub fn pip_key(&self) -> &'static str {
        pip_presets()
            .iter()
            .find(|preset| {
                preset.enabled == self.webcam_enabled
                    && (!preset.enabled || (preset.size == self.webcam_size && preset.corner == self.webcam_position))
            })
            .map(|preset| preset.key)
            .unwrap_or("off")
    }

    pub fn apply_pip(&mut self, key: &str) {
        if let Some(preset) = pip_presets().iter().find(|preset| preset.key == key) {
            self.webcam_enabled = preset.enabled;
            self.webcam_size = preset.size;
            self.webcam_position = preset.corner;
        }
    }
}

impl Corner {
    pub fn overlay_xy(self, margin: i32) -> (String, String) {
        let m = margin.to_string();
        match self {
            Corner::TopLeft => (m.clone(), m),
            Corner::TopRight => (format!("W-w-{margin}"), m),
            Corner::BottomLeft => (m, format!("H-h-{margin}")),
            Corner::BottomRight => (format!("W-w-{margin}"), format!("H-h-{margin}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct Hotkeys {
    pub shot_screen: String,
    pub shot_window: String,
    pub shot_region: String,
    pub record_toggle: String,
    pub record_pause: String,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            shot_screen: "CommandOrControl+Shift+Digit1".into(),
            shot_window: "CommandOrControl+Shift+Digit2".into(),
            shot_region: "CommandOrControl+Shift+Digit3".into(),
            record_toggle: "CommandOrControl+Shift+Digit4".into(),
            record_pause: "CommandOrControl+Shift+Digit5".into(),
        }
    }
}

impl Hotkeys {
    pub fn entries(&self) -> [(&str, &str); 5] {
        [
            ("shotScreen", self.shot_screen.as_str()),
            ("shotWindow", self.shot_window.as_str()),
            ("shotRegion", self.shot_region.as_str()),
            ("recordToggle", self.record_toggle.as_str()),
            ("recordPause", self.record_pause.as_str()),
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub output_dir: String,
    pub locale: String,
    pub video_height: VideoHeight,
    pub fps: u32,
    pub quality: Quality,
    pub show_cursor: bool,
    pub capture_microphone: bool,
    pub capture_system_audio: bool,
    pub webcam_enabled: bool,
    pub webcam_size: WebcamSize,
    pub webcam_position: Corner,
    pub share_direct: bool,
    pub peer_url: String,
    pub server_url: String,
    pub share_token: String,
    pub hotkeys: Hotkeys,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            output_dir: default_output_dir().to_string_lossy().into_owned(),
            locale: "fr".into(),
            video_height: VideoHeight::Native,
            fps: 30,
            quality: Quality::Balanced,
            show_cursor: true,
            capture_microphone: false,
            capture_system_audio: false,
            webcam_enabled: false,
            webcam_size: WebcamSize::Medium,
            webcam_position: Corner::BottomRight,
            share_direct: false,
            peer_url: String::new(),
            server_url: String::new(),
            share_token: String::new(),
            hotkeys: Hotkeys::default(),
        }
    }
}

impl Settings {
    pub fn output_dir_path(&self) -> std::path::PathBuf {
        std::path::PathBuf::from(&self.output_dir)
    }

    pub fn normalize(mut self) -> Self {
        self.locale = normalize_locale(&self.locale);
        self.fps = normalize_fps(self.fps);
        if self.output_dir.trim().is_empty() {
            self.output_dir = default_output_dir().to_string_lossy().into_owned();
        }
        self
    }

    pub fn load(path: &std::path::Path) -> Self {
        let Ok(raw) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        serde_json::from_str::<Settings>(&raw)
            .map(Self::normalize)
            .unwrap_or_else(|_| Self::default())
    }

    pub fn save(&self, path: &std::path::Path) -> Cmd<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io_err(&self.locale, &e))?;
        }
        let body = serde_json::to_string_pretty(self).map_err(|e| {
            CommandError::new("IO", format!("{} {e}", copy::text(&self.locale, "IO")))
        })?;
        std::fs::write(path, body).map_err(|e| io_err(&self.locale, &e))
    }

    pub fn validate_hotkeys(&self) -> Cmd<()> {
        let mut seen = Vec::new();
        for (_name, accel) in self.hotkeys.entries() {
            if HotKey::from_str(accel).is_err() {
                return Err(fail(&self.locale, "HOTKEY_INVALID"));
            }
            if seen.iter().any(|s: &String| s.eq_ignore_ascii_case(accel)) {
                return Err(fail(&self.locale, "HOTKEY_DUPLICATE"));
            }
            seen.push(accel.to_string());
        }
        Ok(())
    }
}

pub fn normalize_locale(value: &str) -> String {
    if value.eq_ignore_ascii_case("en") {
        "en".into()
    } else {
        "fr".into()
    }
}

pub fn normalize_fps(fps: u32) -> u32 {
    if (10..=60).contains(&fps) { fps } else { 30 }
}

pub fn fail(locale: &str, code: &str) -> CommandError {
    CommandError::new(code, copy::text(locale, code))
}

fn io_err(locale: &str, err: &std::io::Error) -> CommandError {
    CommandError::new("IO", format!("{} {err}", copy::text(locale, "IO")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_partial_json_roundtrip() {
        let settings = Settings::default();
        assert_eq!(settings.locale, "fr");
        assert_eq!(settings.fps, 30);
        assert_eq!(settings.quality.crf(), 23);
        assert_eq!(settings.video_height.max_height(), 2160);
        let raw = serde_json::to_string(&settings).unwrap();
        let back: Settings = serde_json::from_str(&raw).unwrap();
        assert_eq!(back.hotkeys, settings.hotkeys);

        let partial: Settings = serde_json::from_str(r#"{"locale":"en","fps":15}"#).unwrap();
        let partial = partial.normalize();
        assert_eq!(partial.locale, "en");
        assert_eq!(partial.fps, 15);
        assert_eq!(partial.quality, Quality::Balanced);
    }

    #[test]
    fn rejects_duplicate_and_unknown_fps_falls_back() {
        assert_eq!(normalize_fps(0), 30);
        assert_eq!(normalize_fps(60), 60);
        let mut settings = Settings::default();
        settings.hotkeys.shot_window = settings.hotkeys.shot_screen.clone();
        assert_eq!(settings.validate_hotkeys().unwrap_err().code, "HOTKEY_DUPLICATE");
        settings.hotkeys = Hotkeys::default();
        settings.hotkeys.record_pause = "not a shortcut".into();
        assert_eq!(settings.validate_hotkeys().unwrap_err().code, "HOTKEY_INVALID");
        settings.hotkeys = Hotkeys::default();
        assert!(settings.validate_hotkeys().is_ok());
    }

    #[test]
    fn pip_presets_apply_size_and_corner() {
        let mut settings = Settings::default();
        assert_eq!(settings.pip_key(), "off");
        settings.apply_pip("l-br");
        assert!(settings.webcam_enabled);
        assert_eq!(settings.webcam_size, WebcamSize::Large);
        assert_eq!(settings.webcam_position, Corner::BottomRight);
        assert_eq!(settings.pip_key(), "l-br");
        settings.apply_pip("s-tl");
        assert_eq!(settings.webcam_size, WebcamSize::Small);
        assert_eq!(settings.webcam_position, Corner::TopLeft);
        settings.apply_pip("off");
        assert!(!settings.webcam_enabled);
        assert_eq!(settings.pip_key(), "off");
    }

    fn corners_emit_overlay_expressions() {
        assert_eq!(Corner::BottomRight.overlay_xy(28), ("W-w-28".into(), "H-h-28".into()));
        assert_eq!(Corner::TopLeft.overlay_xy(12), ("12".into(), "12".into()));
    }
}
