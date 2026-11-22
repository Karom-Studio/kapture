use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::encode::{self, EncodeRequest};
use crate::error::CommandError;
use crate::library::CaptureSource;
use crate::media;
use crate::settings::fail;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Recording,
    Paused,
}

pub struct Session {
    pub phase: Phase,
    pub accumulated: Duration,
    pub segment_started: Option<Instant>,
    pub segments: Vec<PathBuf>,
    pub work_dir: PathBuf,
    pub final_path: PathBuf,
    pub child: Option<Child>,
    pub stderr_path: PathBuf,
    pub request: EncodeRequest,
    pub warnings: Vec<String>,
    pub source: CaptureSource,
}

impl Session {
    pub fn elapsed(&self) -> Duration {
        let extra = self.segment_started.map(|t| t.elapsed()).unwrap_or_default();
        self.accumulated + extra
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
    }

    pub fn phase_name(&self) -> &'static str {
        match self.phase {
            Phase::Recording => "recording",
            Phase::Paused => "paused",
        }
    }
}

pub fn open_session(
    locale: &str,
    request: EncodeRequest,
    work_dir: PathBuf,
    final_path: PathBuf,
    source: CaptureSource,
    warnings: Vec<String>,
) -> Result<Session, CommandError> {
    fs::create_dir_all(&work_dir).map_err(|e| CommandError::new("IO", e.to_string()))?;
    let mut session = Session {
        phase: Phase::Recording,
        accumulated: Duration::ZERO,
        segment_started: None,
        segments: Vec::new(),
        work_dir,
        final_path,
        child: None,
        stderr_path: PathBuf::new(),
        request,
        warnings,
        source,
    };
    session.spawn_segment(locale)?;
    Ok(session)
}

impl Session {
    pub fn spawn_segment(&mut self, locale: &str) -> Result<(), CommandError> {
        let index = self.segments.len();
        let output = self.work_dir.join(format!("segment-{index:03}.mp4"));
        let stderr_path = self.work_dir.join(format!("ffmpeg-{index:03}.log"));
        let mut request = self.request.clone();
        request.output = output.display().to_string();
        let args = encode::build_args(&request);
        let bin = encode::find_tool("ffmpeg").ok_or_else(|| fail(locale, "FFMPEG_MISSING"))?;
        eprintln!("lueur: ffmpeg {}", args.join(" "));
        let stderr = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&stderr_path)
            .map_err(|e| CommandError::new("IO", e.to_string()))?;
        let mut child = Command::new(bin)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr))
            .spawn()
            .map_err(|e| CommandError::new("FFMPEG_SPAWN", format!("{} {e}", crate::copy::text(locale, "FFMPEG_SPAWN"))))?;
        thread::sleep(Duration::from_millis(450));
        if let Ok(Some(_status)) = child.try_wait() {
            let err = fs::read_to_string(&stderr_path).unwrap_or_default();
            return Err(CommandError::new(
                "FFMPEG_FAILED",
                format!("{} {}", crate::copy::text(locale, "FFMPEG_FAILED"), media::tail(&err)),
            ));
        }
        self.child = Some(child);
        self.stderr_path = stderr_path;
        self.segments.push(output);
        self.segment_started = Some(Instant::now());
        self.phase = Phase::Recording;
        Ok(())
    }

    pub fn pause(&mut self, locale: &str) -> Result<(), CommandError> {
        if self.phase != Phase::Recording {
            return Err(fail(locale, "NOT_RECORDING"));
        }
        self.finish_child();
        if let Some(started) = self.segment_started.take() {
            self.accumulated += started.elapsed();
        }
        self.phase = Phase::Paused;
        Ok(())
    }

    pub fn resume(&mut self, locale: &str) -> Result<(), CommandError> {
        if self.phase != Phase::Paused {
            return Err(fail(locale, "NOT_RECORDING"));
        }
        self.spawn_segment(locale)
    }

    pub fn stop(mut self, locale: &str) -> Result<Finished, CommandError> {
        if self.phase == Phase::Recording {
            self.finish_child();
            if let Some(started) = self.segment_started.take() {
                self.accumulated += started.elapsed();
            }
        }
        self.segments.retain(|path| path.is_file() && fs::metadata(path).map(|m| m.len() > 800).unwrap_or(false));
        if self.segments.is_empty() {
            let err = fs::read_to_string(&self.stderr_path).unwrap_or_default();
            let _ = fs::remove_dir_all(&self.work_dir);
            return Err(CommandError::new(
                "FFMPEG_FAILED",
                format!("{} {}", crate::copy::text(locale, "FFMPEG_FAILED"), media::tail(&err)),
            ));
        }
        if let Some(parent) = self.final_path.parent() {
            fs::create_dir_all(parent).map_err(|e| CommandError::new("IO", e.to_string()))?;
        }
        if self.segments.len() == 1 {
            move_file(&self.segments[0], &self.final_path).map_err(|e| CommandError::new("IO", e.to_string()))?;
        } else {
            concat(locale, &self.segments, &self.final_path)?;
        }
        let duration_ms = encode::probe_duration_ms(&self.final_path).unwrap_or_else(|| self.accumulated.as_millis() as u64);
        let (width, height) = encode::probe_video_size(&self.final_path).unwrap_or((self.request.output_w, self.request.output_h));
        let bytes = fs::metadata(&self.final_path).map(|m| m.len()).unwrap_or(0);
        let _ = fs::remove_dir_all(&self.work_dir);
        Ok(Finished {
            path: self.final_path,
            width,
            height,
            duration_ms,
            bytes,
            warnings: self.warnings,
            source: self.source,
        })
    }

    fn finish_child(&mut self) {
        if let Some(mut child) = self.child.take() {
            stop_ffmpeg(&mut child);
        }
    }
}

pub struct Finished {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub duration_ms: u64,
    pub bytes: u64,
    pub warnings: Vec<String>,
    pub source: CaptureSource,
}

pub fn stop_ffmpeg(child: &mut Child) {
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(b"q\n");
        let _ = stdin.flush();
    }
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) if started.elapsed() > Duration::from_secs(3) => {
                #[cfg(unix)]
                unsafe {
                    libc::kill(child.id() as i32, libc::SIGINT);
                }
                #[cfg(not(unix))]
                {
                    let _ = child.kill();
                }
                if started.elapsed() > Duration::from_secs(5) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return;
                }
            }
            Ok(None) => thread::sleep(Duration::from_millis(40)),
            Err(_) => return,
        }
    }
}

fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if from == to {
        return Ok(());
    }
    if fs::rename(from, to).is_err() {
        fs::copy(from, to)?;
        let _ = fs::remove_file(from);
    }
    Ok(())
}

fn concat(locale: &str, segments: &[PathBuf], output: &Path) -> Result<(), CommandError> {
    let list_path = output.with_extension("concat.txt");
    fs::write(&list_path, encode::concat_list(segments)).map_err(|e| CommandError::new("IO", e.to_string()))?;
    let bin = encode::find_tool("ffmpeg").ok_or_else(|| fail(locale, "FFMPEG_MISSING"))?;
    let status = Command::new(bin)
        .args(["-y", "-f", "concat", "-safe", "0", "-i"])
        .arg(&list_path)
        .args(["-c", "copy", "-movflags", "+faststart"])
        .arg(output)
        .status()
        .map_err(|e| CommandError::new("FFMPEG_SPAWN", e.to_string()))?;
    let _ = fs::remove_file(&list_path);
    if status.success() && output.is_file() {
        Ok(())
    } else if let Some(first) = segments.first() {
        move_file(first, output).map_err(|e| CommandError::new("IO", e.to_string()))?;
        Ok(())
    } else {
        Err(fail(locale, "FFMPEG_FAILED"))
    }
}

/// Records a short clip. Used by the app's session and by the Linux smoke test.
pub fn record_for(request: &EncodeRequest, duration: Duration) -> Result<(), String> {
    let bin = encode::find_tool("ffmpeg").ok_or("ffmpeg missing")?;
    let args = encode::build_args(request);
    let mut child = Command::new(bin)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    thread::sleep(duration);
    stop_ffmpeg(&mut child);
    let mut err = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        use std::io::Read;
        let _ = stderr.read_to_string(&mut err);
    }
    let path = Path::new(&request.output);
    if path.is_file() && fs::metadata(path).map(|m| m.len() > 500).unwrap_or(false) {
        Ok(())
    } else {
        Err(format!("empty recording at {} ({})", request.output, media::tail(&err)))
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use crate::encode::Grab;

    #[test]
    fn records_a_short_mp4_on_x11() {
        if std::env::var_os("DISPLAY").is_none() || !encode::ffmpeg_available() {
            return;
        }
        let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into());
        let output = std::env::temp_dir().join(format!("lueur-smoke-{}.mp4", std::process::id()));
        let _ = fs::remove_file(&output);
        let request = EncodeRequest {
            grab: Grab::X11 { display, x: 40, y: 40 },
            capture_w: 320,
            capture_h: 180,
            output_w: 320,
            output_h: 180,
            fps: 15,
            crf: 30,
            draw_cursor: false,
            microphone: None,
            system_audio: None,
            webcam: None,
            output: output.display().to_string(),
        };
        record_for(&request, Duration::from_millis(1500)).expect("record");
        let probed = encode::probe_duration_ms(&output).unwrap_or(0);
        assert!(probed > 300, "duration {probed}");
        let _ = fs::remove_file(&output);
    }
}
