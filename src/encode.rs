use std::path::{Path, PathBuf};
use std::process::Command;

use crate::geom::{self, PxRect};
use crate::settings::Corner;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Grab {
    X11 { display: String, x: i32, y: i32 },
    Gdi { x: i32, y: i32 },
    AvFoundation { screen_index: String, crop: Option<PxRect> },
    /// PipeWire node id from an XDG screencast session. Built and tested;
    /// the portal handshake is not started in this version.
    PipeWire { node: String },
}

#[derive(Debug, Clone)]
pub struct Webcam {
    pub device: String,
    pub diameter: u32,
    pub corner: Corner,
}

#[derive(Debug, Clone)]
pub struct EncodeRequest {
    pub grab: Grab,
    pub capture_w: u32,
    pub capture_h: u32,
    pub output_w: u32,
    pub output_h: u32,
    pub fps: u32,
    pub crf: u32,
    pub draw_cursor: bool,
    pub microphone: Option<String>,
    pub system_audio: Option<String>,
    pub webcam: Option<Webcam>,
    pub output: String,
}

pub fn x11_target(display: &str, x: i32, y: i32) -> String {
    let d = display.trim().strip_prefix("DISPLAY=").unwrap_or(display.trim());
    let d = if d.is_empty() { ":0" } else { d };
    let d = if d.contains('.') { d.to_string() } else { format!("{d}.0") };
    format!("{d}+{x},{y}")
}

pub fn find_tool(name: &str) -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os(if name == "ffmpeg" { "LUEUR_FFMPEG" } else { "LUEUR_FFPROBE" }) {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return Some(path);
        }
    }
    let candidates = [
        format!("/usr/bin/{name}"),
        format!("/usr/local/bin/{name}"),
        format!("/opt/homebrew/bin/{name}"),
        name.to_string(),
    ];
    for candidate in candidates {
        if Command::new(&candidate)
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return Some(PathBuf::from(candidate));
        }
    }
    None
}

pub fn ffmpeg_available() -> bool {
    find_tool("ffmpeg").is_some()
}

pub fn build_args(req: &EncodeRequest) -> Vec<String> {
    let mut args = vec![
        "-y".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "warning".into(),
    ];
    push_video_input(&mut args, req);

    let mic = req.microphone.as_ref().map(|_| 0);
    let mut next = 1u32;
    let mic_idx = req.microphone.as_ref().map(|_| {
        push_audio_input(&mut args, &req.grab, req.microphone.as_deref().unwrap());
        let idx = next;
        next += 1;
        idx
    });
    let _ = mic;
    let sys_idx = req.system_audio.as_ref().map(|_| {
        push_audio_input(&mut args, &req.grab, req.system_audio.as_deref().unwrap());
        let idx = next;
        next += 1;
        idx
    });
    let cam_idx = req.webcam.as_ref().map(|cam| {
        push_camera_input(&mut args, &req.grab, cam, req.fps);
        let idx = next;
        next += 1;
        idx
    });

    let mut filters: Vec<String> = Vec::new();
    let mut video_pad = "0:v".to_string();
    if let Some(chain) = video_chain(req) {
        filters.push(format!("[0:v]{chain}[base]"));
        video_pad = "base".into();
    }
    if let (Some(cam), Some(idx)) = (req.webcam.as_ref(), cam_idx) {
        let d = geom::bubble_diameter(cam.diameter, req.output_h);
        let (x, y) = cam.corner.overlay_xy(28);
        filters.push(format!(
            "[{idx}:v]scale={d}:{d}:force_original_aspect_ratio=increase,crop={d}:{d},format=rgba,geq=r='r(X,Y)':g='g(X,Y)':b='b(X,Y)':a='if(lte(hypot(X-W/2\\,Y-H/2)\\,W/2-1)\\,255\\,0)'[bubble]"
        ));
        filters.push(format!("[{video_pad}][bubble]overlay={x}:{y}[vout]"));
        video_pad = "vout".into();
    }
    let audio_pad = match (mic_idx, sys_idx) {
        (Some(a), Some(b)) => {
            filters.push(format!("[{a}:a][{b}:a]amix=inputs=2:duration=longest:dropout_transition=0[aout]"));
            Some("aout".to_string())
        }
        (Some(a), None) => Some(format!("{a}:a")),
        (None, Some(b)) => Some(format!("{b}:a")),
        (None, None) => None,
    };

    if !filters.is_empty() {
        args.push("-filter_complex".into());
        args.push(filters.join(";"));
    }
    args.push("-map".into());
    args.push(if video_pad.contains(':') { video_pad } else { format!("[{video_pad}]") });
    if let Some(pad) = audio_pad {
        args.push("-map".into());
        args.push(if pad.contains(':') && !pad.starts_with('[') { pad } else if pad.contains(':') { pad } else { format!("[{pad}]") });
    }
    args.extend([
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        "veryfast".into(),
        "-crf".into(),
        req.crf.to_string(),
        "-pix_fmt".into(),
        "yuv420p".into(),
    ]);
    if mic_idx.is_some() || sys_idx.is_some() {
        args.extend(["-c:a".into(), "aac".into(), "-b:a".into(), "160k".into()]);
    }
    args.extend(["-movflags".into(), "+faststart".into(), req.output.clone()]);
    args
}

fn video_chain(req: &EncodeRequest) -> Option<String> {
    let mut parts = Vec::new();
    if let Grab::AvFoundation { crop: Some(crop), .. } = &req.grab {
        parts.push(format!("crop={}:{}:{}:{}", crop.width, crop.height, crop.x, crop.y));
    }
    if req.output_w != req.capture_w || req.output_h != req.capture_h {
        parts.push(format!("scale={}:{}", req.output_w, req.output_h));
    }
    if parts.is_empty() { None } else { Some(parts.join(",")) }
}

fn push_video_input(args: &mut Vec<String>, req: &EncodeRequest) {
    match &req.grab {
        Grab::X11 { display, x, y } => {
            args.extend([
                "-thread_queue_size".into(),
                "1024".into(),
                "-f".into(),
                "x11grab".into(),
                "-framerate".into(),
                req.fps.to_string(),
                "-draw_mouse".into(),
                if req.draw_cursor { "1".into() } else { "0".into() },
                "-video_size".into(),
                format!("{}x{}", req.capture_w, req.capture_h),
                "-i".into(),
                x11_target(display, *x, *y),
            ]);
        }
        Grab::Gdi { x, y } => {
            args.extend([
                "-f".into(),
                "gdigrab".into(),
                "-framerate".into(),
                req.fps.to_string(),
                "-offset_x".into(),
                x.to_string(),
                "-offset_y".into(),
                y.to_string(),
                "-video_size".into(),
                format!("{}x{}", req.capture_w, req.capture_h),
                "-draw_mouse".into(),
                if req.draw_cursor { "1".into() } else { "0".into() },
                "-i".into(),
                "desktop".into(),
            ]);
        }
        Grab::AvFoundation { screen_index, .. } => {
            args.extend([
                "-f".into(),
                "avfoundation".into(),
                "-framerate".into(),
                req.fps.to_string(),
                "-capture_cursor".into(),
                if req.draw_cursor { "1".into() } else { "0".into() },
                "-i".into(),
                format!("{screen_index}:none"),
            ]);
        }
        Grab::PipeWire { node } => {
            args.extend([
                "-f".into(),
                "pipewire".into(),
                "-framerate".into(),
                req.fps.to_string(),
                "-i".into(),
                node.clone(),
            ]);
        }
    }
}

fn push_audio_input(args: &mut Vec<String>, grab: &Grab, device: &str) {
    match grab {
        Grab::X11 { .. } | Grab::PipeWire { .. } => {
            args.extend(["-f".into(), "pulse".into(), "-i".into(), device.into()]);
        }
        Grab::Gdi { .. } => {
            args.extend(["-f".into(), "dshow".into(), "-i".into(), format!("audio={device}")]);
        }
        Grab::AvFoundation { .. } => {
            args.extend(["-f".into(), "avfoundation".into(), "-i".into(), format!("none:{device}")]);
        }
    }
}

fn push_camera_input(args: &mut Vec<String>, grab: &Grab, cam: &Webcam, fps: u32) {
    match grab {
        Grab::X11 { .. } | Grab::PipeWire { .. } => {
            args.extend([
                "-f".into(),
                "v4l2".into(),
                "-framerate".into(),
                fps.to_string(),
                "-i".into(),
                cam.device.clone(),
            ]);
        }
        Grab::Gdi { .. } => {
            args.extend([
                "-f".into(),
                "dshow".into(),
                "-framerate".into(),
                fps.to_string(),
                "-i".into(),
                format!("video={}", cam.device),
            ]);
        }
        Grab::AvFoundation { .. } => {
            args.extend([
                "-f".into(),
                "avfoundation".into(),
                "-framerate".into(),
                fps.to_string(),
                "-i".into(),
                format!("{}:none", cam.device),
            ]);
        }
    }
}

pub fn concat_list(paths: &[PathBuf]) -> String {
    let mut out = String::new();
    for path in paths {
        let text = path.to_string_lossy().replace('\\', "/").replace('\'', r"'\''");
        out.push_str(&format!("file '{text}'\n"));
    }
    out
}

pub fn probe_duration_ms(path: &Path) -> Option<u64> {
    let bin = find_tool("ffprobe")?;
    let output = Command::new(bin)
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "default=nw=1:nk=1"])
        .arg(path)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let secs: f64 = text.trim().parse().ok()?;
    if secs.is_finite() && secs >= 0.0 {
        Some((secs * 1000.0).round() as u64)
    } else {
        None
    }
}

pub fn probe_video_size(path: &Path) -> Option<(u32, u32)> {
    let bin = find_tool("ffprobe")?;
    let output = Command::new(bin)
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height",
            "-of",
            "csv=s=x:p=0",
        ])
        .arg(path)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let (w, h) = text.trim().split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(grab: Grab) -> EncodeRequest {
        EncodeRequest {
            grab,
            capture_w: 640,
            capture_h: 360,
            output_w: 640,
            output_h: 360,
            fps: 30,
            crf: 23,
            draw_cursor: true,
            microphone: None,
            system_audio: None,
            webcam: None,
            output: "out.mp4".into(),
        }
    }

    #[test]
    fn x11_target_adds_screen_zero() {
        assert_eq!(x11_target(":1", 0, 0), ":1.0+0,0");
        assert_eq!(x11_target(":0.0", 12, -4), ":0.0+12,-4");
    }

    #[test]
    fn linux_args_include_x11grab_cursor_and_size() {
        let mut req = base(Grab::X11 { display: ":1".into(), x: 8, y: 16 });
        req.draw_cursor = false;
        let args = build_args(&req);
        let joined = args.join(" ");
        assert!(joined.contains("-f x11grab"));
        assert!(joined.contains("-draw_mouse 0"));
        assert!(joined.contains("-video_size 640x360"));
        assert!(joined.contains("-i :1.0+8,16"));
        assert!(joined.contains("-crf 23"));
        assert!(!joined.contains("-c:a"));
    }

    #[test]
    fn linux_audio_and_bubble_are_in_the_graph() {
        let mut req = base(Grab::X11 { display: ":0".into(), x: 0, y: 0 });
        req.microphone = Some("alsa_input.mic".into());
        req.system_audio = Some("alsa_output.monitor".into());
        req.webcam = Some(Webcam {
            device: "/dev/video0".into(),
            diameter: 180,
            corner: Corner::BottomRight,
        });
        let joined = build_args(&req).join(" ");
        assert!(joined.contains("-f pulse -i alsa_input.mic"));
        assert!(joined.contains("-f pulse -i alsa_output.monitor"));
        assert!(joined.contains("-f v4l2"));
        assert!(joined.contains("amix=inputs=2"));
        assert!(joined.contains("overlay=W-w-28:H-h-28"));
        assert!(joined.contains("geq="));
        assert!(joined.contains("\\,"));
        assert!(joined.contains("-c:a aac"));
    }

    #[test]
    fn windows_and_macos_and_pipewire_commands_are_real() {
        let gdi = build_args(&base(Grab::Gdi { x: 10, y: 20 })).join(" ");
        assert!(gdi.contains("-f gdigrab"));
        assert!(gdi.contains("-offset_x 10"));
        assert!(gdi.contains("-i desktop"));

        let mut mac = base(Grab::AvFoundation {
            screen_index: "1".into(),
            crop: Some(PxRect { x: 4, y: 6, width: 200, height: 100 }),
        });
        mac.output_w = 200;
        mac.output_h = 100;
        mac.capture_w = 200;
        mac.capture_h = 100;
        mac.microphone = Some("0".into());
        let mac_args = build_args(&mac).join(" ");
        assert!(mac_args.contains("-f avfoundation"));
        assert!(mac_args.contains("-capture_cursor 1"));
        assert!(mac_args.contains("-i 1:none"));
        assert!(mac_args.contains("-i none:0"));
        assert!(mac_args.contains("crop=200:100:4:6"));

        let pw = build_args(&base(Grab::PipeWire { node: "42".into() })).join(" ");
        assert!(pw.contains("-f pipewire -framerate 30 -i 42"));
    }

    #[test]
    fn scale_filter_when_capped() {
        let mut req = base(Grab::X11 { display: ":1".into(), x: 0, y: 0 });
        req.capture_w = 1920;
        req.capture_h = 1200;
        req.output_w = 1152;
        req.output_h = 720;
        let joined = build_args(&req).join(" ");
        assert!(joined.contains("scale=1152:720"));
    }

    #[test]
    fn concat_list_uses_forward_slashes() {
        let body = concat_list(&[PathBuf::from(r"C:\films\a.mp4"), PathBuf::from("/tmp/b.mp4")]);
        assert!(body.contains("file 'C:/films/a.mp4'"));
        assert!(body.contains("file '/tmp/b.mp4'"));
    }
}
