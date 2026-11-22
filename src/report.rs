use serde::Serialize;

use crate::platform::{DesktopSession, HostOs};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Slot {
    pub state: String,
    pub label: Option<String>,
}

impl Slot {
    fn ready(label: Option<String>) -> Self {
        Self { state: "ready".into(), label }
    }
    fn missing() -> Self {
        Self { state: "missing".into(), label: None }
    }
    fn needs_permission() -> Self {
        Self { state: "needsPermission".into(), label: None }
    }
    fn unsupported() -> Self {
        Self { state: "unsupported".into(), label: None }
    }
}

#[derive(Debug, Clone)]
pub struct Facts {
    pub os: HostOs,
    pub session: DesktopSession,
    pub ffmpeg: bool,
    pub screen_granted: Option<bool>,
    pub microphones: Vec<String>,
    pub system_audio: Vec<String>,
    pub cameras: Vec<String>,
    pub audio_server: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub session: String,
    pub os: String,
    pub ffmpeg: bool,
    pub can_record: bool,
    pub screen: Slot,
    pub microphone: Slot,
    pub system_audio: Slot,
    pub camera: Slot,
    pub notes: Vec<String>,
}

pub fn build_report(facts: &Facts) -> Report {
    let mut notes = Vec::new();
    let screen = match facts.session {
        DesktopSession::X11 => {
            notes.push("SCREEN_X11".into());
            Slot::ready(None)
        }
        DesktopSession::Wayland => {
            notes.push("SCREEN_WAYLAND".into());
            notes.push("WAYLAND_RECORDING".into());
            Slot::needs_permission()
        }
        DesktopSession::Macos => {
            notes.push("SCREEN_MAC".into());
            notes.push("macosDevices".into());
            match facts.screen_granted {
                Some(true) => Slot::ready(None),
                Some(false) => Slot::needs_permission(),
                None => Slot::needs_permission(),
            }
        }
        DesktopSession::Windows => {
            notes.push("SCREEN_WINDOWS".into());
            Slot::ready(None)
        }
    };

    if !facts.ffmpeg {
        notes.push("FFMPEG_MISSING".into());
    }
    notes.push("cursorVideoOnly".into());

    let microphone = if facts.microphones.is_empty() {
        if !facts.audio_server && matches!(facts.os, HostOs::Linux) {
            notes.push("noAudioServer".into());
        }
        Slot::missing()
    } else {
        Slot::ready(facts.microphones.first().cloned())
    };

    let system_audio = if facts.os == HostOs::Macos && facts.system_audio.is_empty() {
        notes.push("macosSystemAudio".into());
        Slot::unsupported()
    } else if facts.system_audio.is_empty() {
        Slot::missing()
    } else {
        Slot::ready(facts.system_audio.first().cloned())
    };

    let camera = if facts.cameras.is_empty() {
        notes.push("noCamera".into());
        Slot::missing()
    } else {
        Slot::ready(facts.cameras.first().cloned())
    };

    let can_record = facts.ffmpeg && !matches!(facts.session, DesktopSession::Wayland);
    Report {
        session: facts.session.as_str().into(),
        os: facts.os.as_str().into(),
        ffmpeg: facts.ffmpeg,
        can_record,
        screen,
        microphone,
        system_audio,
        camera,
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bare() -> Facts {
        Facts {
            os: HostOs::Linux,
            session: DesktopSession::X11,
            ffmpeg: true,
            screen_granted: None,
            microphones: vec![],
            system_audio: vec![],
            cameras: vec![],
            audio_server: false,
        }
    }

    #[test]
    fn x11_can_record_without_a_portal() {
        let report = build_report(&bare());
        assert!(report.can_record);
        assert_eq!(report.screen.state, "ready");
        assert!(report.notes.iter().any(|n| n == "SCREEN_X11"));
        assert_eq!(report.microphone.state, "missing");
    }

    #[test]
    fn wayland_blocks_recording_and_explains_the_portal() {
        let mut facts = bare();
        facts.session = DesktopSession::Wayland;
        let report = build_report(&facts);
        assert!(!report.can_record);
        assert_eq!(report.screen.state, "needsPermission");
        assert!(report.notes.iter().any(|n| n == "WAYLAND_RECORDING"));
    }

    #[test]
    fn macos_system_audio_is_unsupported_without_a_loopback() {
        let mut facts = bare();
        facts.os = HostOs::Macos;
        facts.session = DesktopSession::Macos;
        facts.screen_granted = Some(false);
        facts.audio_server = true;
        let report = build_report(&facts);
        assert_eq!(report.screen.state, "needsPermission");
        assert_eq!(report.system_audio.state, "unsupported");
        assert!(report.can_record);
    }
}
