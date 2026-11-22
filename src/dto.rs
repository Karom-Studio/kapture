use serde::Serialize;

use crate::library::{CaptureRecord, CaptureSource};
use crate::report::Report;
use crate::screen::MonitorInfo;
use crate::settings::Settings;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    pub level: String,
    pub code: String,
    pub message: String,
}

impl Notice {
    pub fn new(level: &str, code: &str, message: impl Into<String>) -> Self {
        Self { level: level.into(), code: code.into(), message: message.into() }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionPreview {
    pub width: u32,
    pub height: u32,
    pub jpeg_base64: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingStatus {
    pub phase: String,
    pub elapsed_ms: u64,
}

impl RecordingStatus {
    pub fn idle() -> Self {
        Self { phase: "idle".into(), elapsed_ms: 0 }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureDto {
    #[serde(flatten)]
    pub record: CaptureRecord,
    pub thumbnail_base64: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorDto {
    pub id: String,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
}

impl From<MonitorInfo> for MonitorDto {
    fn from(info: MonitorInfo) -> Self {
        Self {
            id: info.id,
            name: info.name,
            x: info.x,
            y: info.y,
            width: info.width,
            height: info.height,
            primary: info.primary,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowDto {
    pub id: String,
    pub title: String,
    pub app_name: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionResult {
    pub mode: Option<String>,
    pub notice: Option<Notice>,
    pub warnings: Vec<Notice>,
    pub preview: Option<RegionPreview>,
    pub recording: RecordingStatus,
    pub item: Option<CaptureDto>,
    pub removed_id: Option<String>,
}

impl ActionResult {
    pub fn bare(recording: RecordingStatus) -> Self {
        Self {
            mode: None,
            notice: None,
            warnings: Vec::new(),
            preview: None,
            recording,
            item: None,
            removed_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    pub settings: Settings,
    pub defaults: Settings,
    pub report: Report,
    pub monitors: Vec<MonitorDto>,
    pub library: Vec<CaptureDto>,
    pub recording: RecordingStatus,
    pub version: String,
    pub os: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSaved {
    pub settings: Settings,
    pub report: Report,
    pub library: Vec<CaptureDto>,
    pub warnings: Vec<Notice>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickWindow {
    pub intent: String,
}

pub fn notice_code(locale: &str, level: &str, code: &str) -> Notice {
    Notice::new(level, code, crate::copy::text(locale, code))
}

#[allow(dead_code)]
pub fn source_label(source: &CaptureSource) -> String {
    source.mode.clone()
}
