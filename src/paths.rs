use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub settings_file: PathBuf,
    pub library_file: PathBuf,
    pub thumbs_dir: PathBuf,
    pub work_dir: PathBuf,
}

impl Paths {
    pub fn new() -> Self {
        let project = directories::ProjectDirs::from("app", "Oeilleton", "oeilleton");
        let config_dir = project
            .as_ref()
            .map(|p| p.config_dir().to_path_buf())
            .unwrap_or_else(|| std::env::temp_dir().join("oeilleton"));
        let work_dir = project
            .as_ref()
            .map(|p| p.cache_dir().to_path_buf())
            .unwrap_or_else(|| std::env::temp_dir().join("oeilleton-cache"));
        Self {
            settings_file: config_dir.join("settings.json"),
            library_file: config_dir.join("library.json"),
            thumbs_dir: config_dir.join("thumbs"),
            config_dir,
            work_dir,
        }
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        fs::create_dir_all(&self.config_dir)?;
        fs::create_dir_all(&self.thumbs_dir)?;
        fs::create_dir_all(&self.work_dir)?;
        Ok(())
    }
}

pub fn default_output_dir() -> PathBuf {
    directories::UserDirs::new()
        .and_then(|dirs| dirs.picture_dir().map(|p| p.join("Oeilleton")))
        .or_else(|| {
            directories::UserDirs::new().map(|dirs| dirs.home_dir().join("Pictures").join("Oeilleton"))
        })
        .unwrap_or_else(|| std::env::temp_dir().join("Oeilleton"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_point_at_oeilleton_folders() {
        let paths = Paths::new();
        assert!(paths.config_dir.ends_with("oeilleton") || paths.config_dir.ends_with("Oeilleton"));
        assert!(paths.settings_file.ends_with("settings.json"));
        assert!(paths.library_file.ends_with("library.json"));
        assert!(default_output_dir().ends_with("Oeilleton"));
    }
}
