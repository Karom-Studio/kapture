use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};

pub fn stamp(now: DateTime<Local>) -> String {
    now.format("%Y-%m-%d_%H%M%S").to_string()
}

pub fn capture_filename(stamp: &str, ext: &str) -> String {
    format!("Oeilleton_{stamp}.{ext}")
}

pub fn next_available(dir: &Path, filename: &str) -> PathBuf {
    let candidate = dir.join(filename);
    if !candidate.exists() {
        return candidate;
    }
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Oeilleton");
    let ext = Path::new(filename)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("bin");
    for n in 2..10_000 {
        let path = dir.join(format!("{stem}-{n}.{ext}"));
        if !path.exists() {
            return path;
        }
    }
    dir.join(format!("{stem}-overflow.{ext}"))
}

pub fn is_managed_capture(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("oeilleton_")
        && (lower.ends_with(".png") || lower.ends_with(".mp4"))
        && !lower.contains(".thumb.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn stamp_and_filename_are_stable() {
        let now = Local.with_ymd_and_hms(2026, 11, 22, 10, 30, 45).unwrap();
        let stamp = stamp(now);
        assert_eq!(stamp, "2026-11-22_103045");
        assert_eq!(capture_filename(&stamp, "png"), "Oeilleton_2026-11-22_103045.png");
        assert_eq!(capture_filename(&stamp, "mp4"), "Oeilleton_2026-11-22_103045.mp4");
    }

    #[test]
    fn next_available_skips_existing_files() {
        let dir = std::env::temp_dir().join(format!("oeilleton-name-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let first = next_available(&dir, "Oeilleton_test.png");
        std::fs::write(&first, b"a").unwrap();
        let second = next_available(&dir, "Oeilleton_test.png");
        assert_ne!(first, second);
        assert!(second.ends_with("Oeilleton_test-2.png"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn managed_names_ignore_foreign_files() {
        assert!(is_managed_capture("Oeilleton_2026-11-22_103045.png"));
        assert!(is_managed_capture("Oeilleton_2026-11-22_103045.mp4"));
        assert!(!is_managed_capture("notes.png"));
        assert!(!is_managed_capture("Oeilleton_x.thumb.png"));
        assert!(!is_managed_capture("Oeilleton_x.txt"));
    }
}
