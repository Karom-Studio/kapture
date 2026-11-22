use std::fs;
use std::path::{Path, PathBuf};

use chrono::Local;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::naming::{self, is_managed_capture};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CaptureSource {
    pub mode: String,
    pub monitor_name: Option<String>,
    pub window_title: Option<String>,
    pub fps: Option<u32>,
    pub with_microphone: bool,
    pub with_system_audio: bool,
    pub with_webcam: bool,
}

impl CaptureSource {
    pub fn image(mode: &str) -> Self {
        Self {
            mode: mode.into(),
            monitor_name: None,
            window_title: None,
            fps: None,
            with_microphone: false,
            with_system_audio: false,
            with_webcam: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CaptureRecord {
    pub id: String,
    pub kind: String,
    pub filename: String,
    pub path: String,
    pub created_at: String,
    pub width: u32,
    pub height: u32,
    pub duration_ms: Option<u64>,
    pub bytes: u64,
    pub source: CaptureSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LibraryDisk {
    version: u32,
    items: Vec<CaptureRecord>,
}

#[derive(Debug, Clone, Default)]
pub struct Library {
    items: Vec<CaptureRecord>,
}

impl Library {
    pub fn load(path: &Path) -> Self {
        let Ok(raw) = fs::read_to_string(path) else {
            return Self::default();
        };
        serde_json::from_str::<LibraryDisk>(&raw)
            .map(|disk| Self { items: disk.items })
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let disk = LibraryDisk { version: 1, items: self.items.clone() };
        fs::write(path, serde_json::to_string_pretty(&disk).unwrap_or_else(|_| "{}".into()))
    }

    pub fn items(&self) -> &[CaptureRecord] {
        &self.items
    }

    pub fn insert(&mut self, record: CaptureRecord) {
        self.items.retain(|item| item.id != record.id && item.path != record.path);
        self.items.insert(0, record);
    }

    pub fn remove(&mut self, id: &str) -> Option<CaptureRecord> {
        let pos = self.items.iter().position(|item| item.id == id)?;
        Some(self.items.remove(pos))
    }

    pub fn get(&self, id: &str) -> Option<&CaptureRecord> {
        self.items.iter().find(|item| item.id == id)
    }

    /// Drop missing files and adopt `Kapture_*` files that appeared in the folder.
    pub fn reconcile(&mut self, output_dir: &Path) {
        self.items.retain(|item| Path::new(&item.path).is_file());
        let Ok(entries) = fs::read_dir(output_dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !is_managed_capture(name) {
                continue;
            }
            let path_str = path.to_string_lossy().to_string();
            if self.items.iter().any(|item| item.path == path_str) {
                continue;
            }
            let kind = if name.to_ascii_lowercase().ends_with(".mp4") {
                "recording"
            } else {
                "screenshot"
            };
            let bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
            let (width, height) = if kind == "screenshot" {
                image::image_dimensions(&path).unwrap_or((0, 0))
            } else {
                (0, 0)
            };
            self.items.push(CaptureRecord {
                id: Uuid::new_v4().to_string(),
                kind: kind.into(),
                filename: name.into(),
                path: path_str,
                created_at: Local::now().to_rfc3339(),
                width,
                height,
                duration_ms: None,
                bytes,
                source: CaptureSource::image(if kind == "recording" { "recording" } else { "file" }),
            });
        }
        self.items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    }
}

pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

pub fn stamp_now() -> String {
    naming::stamp(Local::now())
}

pub fn thumb_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.jpg"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kapture-{name}-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn record(path: &Path) -> CaptureRecord {
        CaptureRecord {
            id: "one".into(),
            kind: "screenshot".into(),
            filename: "Kapture_a.png".into(),
            path: path.display().to_string(),
            created_at: "2026-11-22T10:00:00+00:00".into(),
            width: 10,
            height: 8,
            duration_ms: None,
            bytes: 4,
            source: CaptureSource::image("screen"),
        }
    }

    #[test]
    fn insert_remove_and_drop_missing_files() {
        let dir = temp_dir("lib");
        let file = dir.join("Kapture_a.png");
        fs::write(&file, b"png").unwrap();
        let mut library = Library::default();
        library.insert(record(&file));
        assert_eq!(library.items().len(), 1);
        fs::remove_file(&file).unwrap();
        library.reconcile(&dir);
        assert!(library.items().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn reconcile_adopts_managed_names_only() {
        let dir = temp_dir("scan");
        fs::write(dir.join("Kapture_2026-11-22_103045.png"), b"png").unwrap();
        fs::write(dir.join("notes.png"), b"nope").unwrap();
        let mut library = Library::default();
        library.reconcile(&dir);
        assert_eq!(library.items().len(), 1);
        assert_eq!(library.items()[0].filename, "Kapture_2026-11-22_103045.png");
        let id = library.items()[0].id.clone();
        let removed = library.remove(&id).unwrap();
        assert!(removed.path.contains("Kapture_"));
        let _ = fs::remove_dir_all(&dir);
    }
}
