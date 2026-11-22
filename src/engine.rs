use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use image::RgbaImage;

use crate::devices::{self, DeviceSet};
use crate::dto::{notice_code, ActionResult, Bootstrap, CaptureDto, MonitorDto, Notice, RecordingStatus, WindowDto};
use crate::encode::{self, EncodeRequest, Grab, Webcam};
use crate::error::{Cmd, CommandError};
use crate::geom::{self, PxRect, ScreenRect};
use crate::library::{self, CaptureRecord, CaptureSource};
use crate::media;
use crate::naming::{capture_filename, next_available};
use crate::paths::Paths;
use crate::platform::{DesktopSession, HostOs};
use crate::report::{self, Facts};
use crate::screen::{self, MonitorInfo};
use crate::session::{self, Session};
use crate::settings::{fail, Settings};

pub struct PendingRegion {
    pub image: RgbaImage,
    pub origin_x: i32,
    pub origin_y: i32,
    pub monitor_name: String,
    pub monitor_width: u32,
}

pub struct Model {
    pub paths: Paths,
    pub settings: Settings,
    pub library: library::Library,
    pub session: Option<Session>,
    pub pending: Option<PendingRegion>,
}

impl Model {
    pub fn boot() -> Self {
        let paths = Paths::new();
        let _ = paths.ensure();
        let settings = Settings::load(&paths.settings_file).normalize();
        let _ = fs::create_dir_all(settings.output_dir_path());
        let mut library = library::Library::load(&paths.library_file);
        library.reconcile(&settings.output_dir_path());
        let _ = library.save(&paths.library_file);
        Self { paths, settings, library, session: None, pending: None }
    }
}

pub struct Engine {
    model: Mutex<Model>,
}

impl Engine {
    pub fn new() -> Self {
        Self { model: Mutex::new(Model::boot()) }
    }

    fn lock(&self) -> MutexGuard<'_, Model> {
        self.model.lock().unwrap_or_else(|err| err.into_inner())
    }

    pub fn locale(&self) -> String {
        self.lock().settings.locale.clone()
    }

    pub fn bootstrap(&self) -> Bootstrap {
        let model = self.lock();
        let monitors = screen::list_monitors(&model.settings.locale).unwrap_or_default();
        Bootstrap {
            settings: model.settings.clone(),
            defaults: Settings::default(),
            report: report_for(&model.settings),
            monitors: monitors.into_iter().map(MonitorDto::from).collect(),
            library: dtos(&model),
            recording: status_of(&model),
            version: env!("CARGO_PKG_VERSION").into(),
            os: HostOs::current().as_str().into(),
        }
    }

    pub fn save_settings(&self, incoming: Settings) -> Cmd<Bootstrap> {
        let mut next = incoming.normalize();
        next.validate_hotkeys()?;
        {
            let mut model = self.lock();
            fs::create_dir_all(next.output_dir_path()).map_err(|e| CommandError::new("IO", e.to_string()))?;
            next.save(&model.paths.settings_file)?;
            model.settings = next;
            let dir = model.settings.output_dir_path();
            model.library.reconcile(&dir);
            let _ = model.library.save(&model.paths.library_file);
        }
        Ok(self.bootstrap())
    }

    pub fn capture_screen(&self, monitor_id: Option<String>) -> Cmd<ActionResult> {
        self.ensure_idle()?;
        let locale = self.locale();
        let (info, image) = screen::capture_monitor(&locale, monitor_id.as_deref())?;
        let mut source = CaptureSource::image("screen");
        source.monitor_name = Some(info.name);
        self.persist_image(image, source, true)
    }

    pub fn capture_window(&self, window_id: &str) -> Cmd<ActionResult> {
        self.ensure_idle()?;
        let locale = self.locale();
        let (info, image) = screen::capture_window(&locale, window_id)?;
        let mut source = CaptureSource::image("window");
        source.window_title = Some(info.title);
        self.persist_image(image, source, true)
    }

    pub fn list_windows(&self) -> Cmd<Vec<WindowDto>> {
        let locale = self.locale();
        Ok(screen::list_windows(&locale)?
            .into_iter()
            .map(|info| WindowDto {
                id: info.id,
                title: info.title,
                app_name: info.app_name,
                width: info.width,
                height: info.height,
            })
            .collect())
    }

    pub fn begin_region(&self) -> Cmd<(u32, u32, RgbaImage)> {
        self.ensure_idle()?;
        let locale = self.locale();
        let (info, image) = screen::capture_monitor(&locale, None)?;
        let width = image.width();
        let height = image.height();
        self.lock().pending = Some(PendingRegion {
            image: image.clone(),
            origin_x: info.x,
            origin_y: info.y,
            monitor_name: info.name,
            monitor_width: info.width,
        });
        Ok((width, height, image))
    }

    pub fn cancel_region(&self) {
        self.lock().pending = None;
    }

    pub fn complete_region(&self, x: i32, y: i32, width: i32, height: i32, record: bool) -> Cmd<ActionResult> {
        let locale = self.locale();
        let pending = self.lock().pending.take().ok_or_else(|| fail(&locale, "NO_PREVIEW"))?;
        let Some(rect) = geom::clamp_rect(x, y, width, height, pending.image.width(), pending.image.height()) else {
            self.lock().pending = Some(pending);
            return Err(fail(&locale, "REGION_TOO_SMALL"));
        };
        if record {
            if rect.width < 16 || rect.height < 16 {
                self.lock().pending = Some(pending);
                return Err(fail(&locale, "REGION_TOO_SMALL"));
            }
            let screen_rect = ScreenRect {
                x: pending.origin_x + rect.x as i32,
                y: pending.origin_y + rect.y as i32,
                width: rect.width,
                height: rect.height,
            };
            let mut source = CaptureSource::image("region");
            source.monitor_name = Some(pending.monitor_name.clone());
            let anchor = MonitorInfo {
                id: String::new(),
                name: pending.monitor_name,
                x: pending.origin_x,
                y: pending.origin_y,
                width: pending.monitor_width,
                height: pending.image.height(),
                primary: true,
            };
            return self.start_rect(screen_rect, source, &anchor, false);
        }
        let image = image::imageops::crop_imm(&pending.image, rect.x, rect.y, rect.width, rect.height).to_image();
        let mut source = CaptureSource::image("region");
        source.monitor_name = Some(pending.monitor_name);
        self.persist_image(image, source, true)
    }

    pub fn start_screen(&self, monitor_id: Option<String>) -> Cmd<ActionResult> {
        self.ensure_idle()?;
        let locale = self.locale();
        let monitors = screen::list_monitors(&locale)?;
        let info = pick(&monitors, monitor_id.as_deref()).clone();
        let rect = ScreenRect { x: info.x, y: info.y, width: info.width, height: info.height };
        let mut source = CaptureSource::image("screen");
        source.monitor_name = Some(info.name.clone());
        self.start_rect(rect, source, &info, true)
    }

    pub fn start_window(&self, window_id: &str) -> Cmd<ActionResult> {
        self.ensure_idle()?;
        let locale = self.locale();
        let info = screen::window_bounds(&locale, window_id)?;
        let rect = ScreenRect { x: info.x, y: info.y, width: info.width, height: info.height };
        let mut source = CaptureSource::image("window");
        source.window_title = Some(info.title);
        let monitors = screen::list_monitors(&locale).unwrap_or_default();
        let anchor = monitors
            .into_iter()
            .find(|m| info.x >= m.x && info.y >= m.y && info.x < m.x + m.width as i32 && info.y < m.y + m.height as i32)
            .unwrap_or(MonitorInfo {
                id: String::new(),
                name: String::new(),
                x: 0,
                y: 0,
                width: 1280,
                height: 720,
                primary: true,
            });
        self.start_rect(rect, source, &anchor, false)
    }

    pub fn pause(&self) -> Cmd<RecordingStatus> {
        let locale = self.locale();
        let mut model = self.lock();
        let Some(session) = model.session.as_mut() else {
            return Err(fail(&locale, "NOT_RECORDING"));
        };
        session.pause(&locale)?;
        Ok(status_of(&model))
    }

    pub fn resume(&self) -> Cmd<RecordingStatus> {
        let locale = self.locale();
        let mut model = self.lock();
        let Some(session) = model.session.as_mut() else {
            return Err(fail(&locale, "NOT_RECORDING"));
        };
        session.resume(&locale)?;
        Ok(status_of(&model))
    }

    pub fn stop(&self) -> Cmd<ActionResult> {
        let locale = self.locale();
        let session = self.lock().session.take().ok_or_else(|| fail(&locale, "NOT_RECORDING"))?;
        let finished = session.stop(&locale)?;
        let mut model = self.lock();
        let record = finish_record(&model, &finished);
        let dto = dto_for(&model, &record);
        model.library.insert(record);
        let _ = model.library.save(&model.paths.library_file);
        let warnings = finished
            .warnings
            .iter()
            .map(|code| notice_code(&locale, "warn", code))
            .collect();
        Ok(ActionResult {
            mode: Some("browse".into()),
            notice: Some(notice_code(&locale, "success", "RECORD_SAVED")),
            warnings,
            preview: None,
            recording: RecordingStatus::idle(),
            item: Some(dto),
            removed_id: None,
        })
    }

    pub fn toggle(&self) -> Cmd<ActionResult> {
        if self.lock().session.is_some() { self.stop() } else { self.start_screen(None) }
    }

    pub fn open_capture(&self, id: &str) -> Cmd<()> {
        let model = self.lock();
        let record = model.library.get(id).ok_or_else(|| fail(&model.settings.locale, "UNKNOWN"))?;
        open::that(&record.path).map_err(|e| CommandError::new("IO", e.to_string()))?;
        Ok(())
    }

    pub fn reveal_capture(&self, id: &str) -> Cmd<()> {
        let model = self.lock();
        let locale = model.settings.locale.clone();
        let record = model.library.get(id).ok_or_else(|| fail(&locale, "UNKNOWN"))?;
        let path = PathBuf::from(&record.path);
        drop(model);
        let parent = path.parent().unwrap_or(path.as_path());
        let (program, args): (&str, Vec<String>) = match HostOs::current() {
            HostOs::Linux => ("xdg-open", vec![parent.display().to_string()]),
            HostOs::Macos => ("open", vec!["-R".into(), path.display().to_string()]),
            HostOs::Windows => ("explorer", vec![format!("/select,{}", path.display())]),
        };
        std::process::Command::new(program).args(args).spawn().map_err(|e| CommandError::new("IO", e.to_string()))?;
        Ok(())
    }

    pub fn delete_capture(&self, id: &str) -> Cmd<String> {
        let mut model = self.lock();
        let locale = model.settings.locale.clone();
        let Some(record) = model.library.remove(id) else {
            return Err(fail(&locale, "UNKNOWN"));
        };
        let _ = fs::remove_file(&record.path);
        let _ = fs::remove_file(library::thumb_path(&model.paths.thumbs_dir, &record.id));
        let _ = model.library.save(&model.paths.library_file);
        Ok(id.to_string())
    }

    pub fn recording(&self) -> RecordingStatus {
        status_of(&self.lock())
    }

    pub fn hotkeys(&self) -> crate::settings::Hotkeys {
        self.lock().settings.hotkeys.clone()
    }

    fn start_rect(&self, rect: ScreenRect, mut source: CaptureSource, anchor: &MonitorInfo, full: bool) -> Cmd<ActionResult> {
        let locale = self.locale();
        if matches!(DesktopSession::detect(), DesktopSession::Wayland) {
            return Err(fail(&locale, "WAYLAND_RECORDING"));
        }
        if !encode::ffmpeg_available() {
            return Err(fail(&locale, "FFMPEG_MISSING"));
        }
        let settings = self.lock().settings.clone();
        let even = geom::even_screen_rect(rect);
        if even.width < 16 || even.height < 16 {
            return Err(fail(&locale, "REGION_TOO_SMALL"));
        }
        let (output_w, output_h) = geom::fit_within(even.width, even.height, Some(settings.video_height.max_height()));
        let devices = devices::probe(HostOs::current());
        let (microphone, system_audio, webcam, warnings) = pick_devices(&settings, &devices);
        source.fps = Some(settings.fps);
        source.with_microphone = microphone.is_some();
        source.with_system_audio = system_audio.is_some();
        source.with_webcam = webcam.is_some();
        let grab = match HostOs::current() {
            HostOs::Linux => Grab::X11 {
                display: std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into()),
                x: even.x,
                y: even.y,
            },
            HostOs::Windows => Grab::Gdi { x: even.x, y: even.y },
            HostOs::Macos => Grab::AvFoundation {
                screen_index: devices.screens.first().map(|d| d.id.clone()).unwrap_or_else(|| "1".into()),
                crop: if full {
                    None
                } else {
                    Some(PxRect {
                        x: (even.x - anchor.x).max(0) as u32,
                        y: (even.y - anchor.y).max(0) as u32,
                        width: even.width,
                        height: even.height,
                    })
                },
            },
        };
        let final_path = next_available(&settings.output_dir_path(), &capture_filename(&library::stamp_now(), "mp4"));
        let work_dir = self.lock().paths.work_dir.join(format!("rec-{}", library::new_id()));
        let request = EncodeRequest {
            grab,
            capture_w: even.width,
            capture_h: even.height,
            output_w,
            output_h,
            fps: settings.fps,
            crf: settings.quality.crf(),
            draw_cursor: settings.show_cursor,
            microphone,
            system_audio,
            webcam,
            output: String::new(),
        };
        let session = session::open_session(&locale, request, work_dir, final_path, source, warnings.clone())?;
        self.lock().session = Some(session);
        Ok(ActionResult {
            mode: Some("recording".into()),
            notice: Some(notice_code(&locale, "success", "RECORD_STARTED")),
            warnings: warnings.into_iter().map(|code| notice_code(&locale, "warn", &code)).collect(),
            preview: None,
            recording: self.recording(),
            item: None,
            removed_id: None,
        })
    }

    fn persist_image(&self, image: RgbaImage, source: CaptureSource, copy: bool) -> Cmd<ActionResult> {
        let mut model = self.lock();
        let locale = model.settings.locale.clone();
        let dir = model.settings.output_dir_path();
        fs::create_dir_all(&dir).map_err(|e| CommandError::new("IO", e.to_string()))?;
        let path = next_available(&dir, &capture_filename(&library::stamp_now(), "png"));
        media::write_png(&image, &path).map_err(|e| CommandError::new("IO", e))?;
        let id = library::new_id();
        let _ = media::write_thumb(&image, &library::thumb_path(&model.paths.thumbs_dir, &id));
        let mut warnings = Vec::new();
        if copy && media::copy_image(&image).is_err() {
            warnings.push(notice_code(&locale, "warn", "CLIPBOARD"));
        }
        let record = CaptureRecord {
            id,
            kind: "screenshot".into(),
            filename: path.file_name().and_then(|n| n.to_str()).unwrap_or("capture.png").into(),
            path: path.display().to_string(),
            created_at: chrono::Local::now().to_rfc3339(),
            width: image.width(),
            height: image.height(),
            duration_ms: None,
            bytes: fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
            source,
        };
        let dto = dto_for(&model, &record);
        model.library.insert(record);
        let _ = model.library.save(&model.paths.library_file);
        Ok(ActionResult {
            mode: Some("browse".into()),
            notice: Some(notice_code(&locale, "success", "SHOT_SAVED")),
            warnings,
            preview: None,
            recording: status_of(&model),
            item: Some(dto),
            removed_id: None,
        })
    }

    fn ensure_idle(&self) -> Cmd<()> {
        let model = self.lock();
        if model.session.is_some() || model.pending.is_some() {
            Err(fail(&model.settings.locale, "BUSY"))
        } else {
            Ok(())
        }
    }

    pub fn finalize(&self) {
        let mut model = self.lock();
        let Some(session) = model.session.take() else { return };
        let locale = model.settings.locale.clone();
        if let Ok(finished) = session.stop(&locale) {
            let record = finish_record(&model, &finished);
            model.library.insert(record);
            let _ = model.library.save(&model.paths.library_file);
        }
    }
}

fn finish_record(model: &Model, finished: &session::Finished) -> CaptureRecord {
    let id = library::new_id();
    if let Some(bin) = encode::find_tool("ffmpeg") {
        let _ = media::video_thumb(&bin, &finished.path, &library::thumb_path(&model.paths.thumbs_dir, &id));
    }
    CaptureRecord {
        id,
        kind: "recording".into(),
        filename: finished.path.file_name().and_then(|n| n.to_str()).unwrap_or("film.mp4").into(),
        path: finished.path.display().to_string(),
        created_at: chrono::Local::now().to_rfc3339(),
        width: finished.width,
        height: finished.height,
        duration_ms: Some(finished.duration_ms),
        bytes: finished.bytes,
        source: finished.source.clone(),
    }
}

fn dto_for(model: &Model, record: &CaptureRecord) -> CaptureDto {
    CaptureDto {
        thumbnail_base64: media::read_base64(&library::thumb_path(&model.paths.thumbs_dir, &record.id)),
        record: record.clone(),
    }
}

fn dtos(model: &Model) -> Vec<CaptureDto> {
    model.library.items().iter().map(|record| dto_for(model, record)).collect()
}

fn status_of(model: &Model) -> RecordingStatus {
    match &model.session {
        Some(session) => RecordingStatus { phase: session.phase_name().into(), elapsed_ms: session.elapsed_ms() },
        None => RecordingStatus::idle(),
    }
}

fn report_for(settings: &Settings) -> crate::report::Report {
    let devices = devices::probe(HostOs::current());
    report::build_report(&Facts {
        os: HostOs::current(),
        session: DesktopSession::detect(),
        ffmpeg: encode::ffmpeg_available(),
        screen_granted: crate::platform::screen_access_granted(),
        microphones: DeviceSet::labels(&devices.microphones),
        system_audio: DeviceSet::labels(&devices.system_audio),
        cameras: DeviceSet::labels(&devices.cameras),
        audio_server: devices.audio_server,
    })
}

fn pick<'a>(monitors: &'a [MonitorInfo], id: Option<&str>) -> &'a MonitorInfo {
    id.and_then(|id| monitors.iter().find(|m| m.id == id))
        .or_else(|| monitors.iter().find(|m| m.primary))
        .unwrap_or(&monitors[0])
}

fn pick_devices(settings: &Settings, devices: &DeviceSet) -> (Option<String>, Option<String>, Option<Webcam>, Vec<String>) {
    let mut warnings = Vec::new();
    let microphone = if settings.capture_microphone {
        devices.microphones.first().map(|d| d.id.clone()).or_else(|| {
            warnings.push("NO_MIC".into());
            None
        })
    } else {
        None
    };
    let system_audio = if settings.capture_system_audio {
        devices.system_audio.first().map(|d| d.id.clone()).or_else(|| {
            warnings.push("NO_SYSTEM_AUDIO".into());
            None
        })
    } else {
        None
    };
    let webcam = if settings.webcam_enabled {
        devices.cameras.first().map(|d| Webcam {
            device: d.id.clone(),
            diameter: settings.webcam_size.px(),
            corner: settings.webcam_position,
        }).or_else(|| {
            warnings.push("NO_CAMERA".into());
            None
        })
    } else {
        None
    };
    (microphone, system_audio, webcam, warnings)
}

pub fn decode_thumb(b64: &str) -> Option<RgbaImage> {
    let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64).ok()?;
    image::load_from_memory(&bytes).ok().map(|img| img.to_rgba8())
}

#[allow(dead_code)]
fn _path(path: &Path) -> &Path { path }
