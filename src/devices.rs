use std::path::Path;
use std::process::Command;

use serde::Serialize;

use crate::platform::HostOs;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Default)]
pub struct DeviceSet {
    pub microphones: Vec<Device>,
    pub system_audio: Vec<Device>,
    pub cameras: Vec<Device>,
    pub screens: Vec<Device>,
    pub audio_server: bool,
}

impl DeviceSet {
    pub fn labels(list: &[Device]) -> Vec<String> {
        list.iter().map(|d| d.label.clone()).collect()
    }
}

pub fn probe(os: HostOs) -> DeviceSet {
    match os {
        HostOs::Linux => probe_linux(),
        HostOs::Windows => probe_ffmpeg_list(os, &["-hide_banner", "-list_devices", "true", "-f", "dshow", "-i", "dummy"]),
        HostOs::Macos => probe_ffmpeg_list(os, &["-hide_banner", "-f", "avfoundation", "-list_devices", "true", "-i", ""]),
    }
}

fn probe_linux() -> DeviceSet {
    let mut set = DeviceSet {
        cameras: linux_cameras(),
        ..DeviceSet::default()
    };
    let Ok(output) = Command::new("pactl").args(["list", "short", "sources"]).output() else {
        return set;
    };
    if !output.status.success() {
        return set;
    }
    set.audio_server = true;
    let text = String::from_utf8_lossy(&output.stdout);
    let (mics, monitors) = parse_pactl_short(&text);
    set.microphones = mics;
    set.system_audio = monitors;
    set
}

pub fn linux_cameras() -> Vec<Device> {
    let Ok(entries) = std::fs::read_dir("/dev") else {
        return Vec::new();
    };
    let mut cams = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("video") {
            continue;
        }
        let sys = Path::new("/sys/class/video4linux").join(&name).join("name");
        let label = std::fs::read_to_string(sys)
            .unwrap_or_default()
            .trim()
            .to_string();
        if label.to_lowercase().contains("metadata") {
            continue;
        }
        cams.push(Device {
            id: format!("/dev/{name}"),
            label: if label.is_empty() { name } else { label },
        });
    }
    cams.sort_by(|a, b| a.id.cmp(&b.id));
    cams
}

pub fn parse_pactl_short(text: &str) -> (Vec<Device>, Vec<Device>) {
    let mut mics = Vec::new();
    let mut monitors = Vec::new();
    for line in text.lines() {
        let mut parts = line.split('\t');
        let _index = parts.next();
        let Some(name) = parts.next() else { continue };
        if name.is_empty() {
            continue;
        }
        let device = Device { id: name.to_string(), label: name.to_string() };
        if name.ends_with(".monitor") {
            monitors.push(device);
        } else {
            mics.push(device);
        }
    }
    (mics, monitors)
}

fn probe_ffmpeg_list(os: HostOs, args: &[&str]) -> DeviceSet {
    let Some(bin) = crate::encode::find_tool("ffmpeg") else {
        return DeviceSet::default();
    };
    let Ok(output) = Command::new(bin).args(args).output() else {
        return DeviceSet::default();
    };
    let mut text = String::from_utf8_lossy(&output.stderr).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    match os {
        HostOs::Windows => parse_dshow(&text),
        HostOs::Macos => parse_avfoundation(&text),
        HostOs::Linux => DeviceSet::default(),
    }
}

pub fn parse_dshow(text: &str) -> DeviceSet {
    let mut set = DeviceSet { audio_server: true, ..DeviceSet::default() };
    let mut section = "";
    for line in text.lines() {
        let lower = line.to_lowercase();
        if lower.contains("video devices") {
            section = "video";
            continue;
        }
        if lower.contains("audio devices") {
            section = "audio";
            continue;
        }
        let Some(name) = quoted(line) else { continue };
        if name.to_lowercase().contains("alternative name") {
            continue;
        }
        match section {
            "video" => set.cameras.push(Device { id: name.clone(), label: name }),
            "audio" => {
                let device = Device { id: name.clone(), label: name.clone() };
                if is_loopback(&name) {
                    set.system_audio.push(device);
                } else {
                    set.microphones.push(device);
                }
            }
            _ => {}
        }
    }
    set
}

pub fn parse_avfoundation(text: &str) -> DeviceSet {
    let mut set = DeviceSet { audio_server: true, ..DeviceSet::default() };
    let mut section = "";
    for line in text.lines() {
        let lower = line.to_lowercase();
        if lower.contains("video devices") {
            section = "video";
            continue;
        }
        if lower.contains("audio devices") {
            section = "audio";
            continue;
        }
        let Some((index, name)) = bracket_device(line) else { continue };
        match section {
            "video" => {
                if name.to_lowercase().contains("capture screen") {
                    set.screens.push(Device { id: index, label: name });
                } else {
                    set.cameras.push(Device { id: index, label: name });
                }
            }
            "audio" => {
                let device = Device { id: index, label: name.clone() };
                if is_loopback(&name) {
                    set.system_audio.push(device);
                } else {
                    set.microphones.push(device);
                }
            }
            _ => {}
        }
    }
    set
}

fn quoted(line: &str) -> Option<String> {
    let start = line.find('"')?;
    let end = line[start + 1..].find('"')? + start + 1;
    Some(line[start + 1..end].to_string())
}

fn bracket_device(line: &str) -> Option<(String, String)> {
    let mut rest = line;
    while let Some(start) = rest.find('[') {
        let after = &rest[start + 1..];
        let Some(end) = after.find(']') else { break };
        let index = after[..end].trim();
        if !index.is_empty() && index.chars().all(|c| c.is_ascii_digit()) {
            let name = after[end + 1..].trim().trim_matches('"').trim();
            if !name.is_empty() {
                return Some((index.to_string(), name.to_string()));
            }
        }
        rest = &after[end + 1..];
    }
    None
}

fn is_loopback(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("stereo mix")
        || n.contains("mixage stéréo")
        || n.contains("mixage stereo")
        || n.contains("what u hear")
        || n.contains("loopback")
        || n.contains("blackhole")
        || n.contains("soundflower")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pactl_splits_mics_and_monitors() {
        let text = "\
0\talsa_output.pci.analog-stereo.monitor\tmodule\ts16le\tRUNNING
1\talsa_input.pci.analog-stereo\tmodule\ts16le\tSUSPENDED
";
        let (mics, monitors) = parse_pactl_short(text);
        assert_eq!(monitors.len(), 1);
        assert!(monitors[0].id.ends_with(".monitor"));
        assert_eq!(mics.len(), 1);
        assert_eq!(mics[0].id, "alsa_input.pci.analog-stereo");
    }

    #[test]
    fn dshow_parser_finds_camera_mic_and_stereo_mix() {
        let text = r#"
[dshow] DirectShow video devices
[dshow]  "Integrated Camera"
[dshow]     Alternative name "@device"
[dshow] DirectShow audio devices
[dshow]  "Microphone Array"
[dshow]  "Stereo Mix (Realtek)"
"#;
        let set = parse_dshow(text);
        assert_eq!(set.cameras[0].id, "Integrated Camera");
        assert_eq!(set.microphones[0].id, "Microphone Array");
        assert_eq!(set.system_audio[0].label, "Stereo Mix (Realtek)");
    }

    #[test]
    fn avfoundation_parser_splits_screens_and_blackhole() {
        let text = r#"
[AVFoundation] AVFoundation video devices:
[AVFoundation] [0] FaceTime HD Camera
[AVFoundation] [1] Capture screen 0
[AVFoundation] AVFoundation audio devices:
[AVFoundation] [0] MacBook Pro Microphone
[AVFoundation] [1] BlackHole 2ch
"#;
        let set = parse_avfoundation(text);
        assert_eq!(set.cameras[0].id, "0");
        assert_eq!(set.screens[0].id, "1");
        assert_eq!(set.microphones[0].label, "MacBook Pro Microphone");
        assert_eq!(set.system_audio[0].id, "1");
    }
}
