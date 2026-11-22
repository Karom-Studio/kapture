pub fn text(locale: &str, code: &str) -> String {
    let en = locale.eq_ignore_ascii_case("en");
    let msg = match code {
        "SHOT_SAVED" => if en { "Picture saved and copied." } else { "Photo enregistrée et copiée." },
        "RECORD_SAVED" => if en { "Recording saved." } else { "Film enregistré." },
        "RECORD_STARTED" => if en { "Recording." } else { "Enregistrement en cours." },
        "RECORD_PAUSED" => if en { "Paused." } else { "En pause." },
        "RECORD_RESUMED" => if en { "Recording again." } else { "Reprise de l'enregistrement." },
        "NO_MONITOR" => if en { "No display detected." } else { "Aucun écran détecté." },
        "FFMPEG_MISSING" => if en {
            "FFmpeg was not found. Install it to record. Screenshots do not need it."
        } else {
            "FFmpeg est introuvable. Installez-le pour filmer. Les photos n'en ont pas besoin."
        },
        "FFMPEG_FAILED" => if en { "FFmpeg stopped." } else { "FFmpeg s'est arrêté." },
        "FFMPEG_SPAWN" => if en { "Could not start FFmpeg." } else { "Impossible de lancer FFmpeg." },
        "REGION_TOO_SMALL" => if en { "That area is too small." } else { "Cette zone est trop petite." },
        "WINDOW_NOT_FOUND" => if en { "That window is gone." } else { "Cette fenêtre a disparu." },
        "WINDOW_MINIMIZED" => if en { "That window is minimized." } else { "Cette fenêtre est réduite." },
        "ALREADY_RECORDING" => if en { "A recording is already running." } else { "Un enregistrement est déjà en cours." },
        "NOT_RECORDING" => if en { "Nothing is recording." } else { "Aucun enregistrement en cours." },
        "WAYLAND_RECORDING" => if en {
            "On a pure Wayland session, continuous recording through the PipeWire portal is not wired up yet. Screenshots still use the portal. Use an X11 or XWayland session to record."
        } else {
            "En session Wayland pure, le film continu via le portail PipeWire n'est pas encore relié. Les photos passent par le portail. Pour filmer, ouvrez une session X11 ou XWayland."
        },
        "CAPTURE_FAILED" => if en { "The capture failed." } else { "La capture a échoué." },
        "PERMISSION" => if en {
            "Screen access was denied. Allow Œilleton in the system privacy settings, then try again."
        } else {
            "L'accès à l'écran a été refusé. Autorisez Œilleton dans les réglages de confidentialité du système, puis réessayez."
        },
        "HOTKEY_INVALID" => if en { "That shortcut is not valid." } else { "Ce raccourci n'est pas valide." },
        "HOTKEY_DUPLICATE" => if en { "Each shortcut must be different." } else { "Chaque raccourci doit être différent." },
        "IO" => if en { "Could not write the file." } else { "Impossible d'écrire le fichier." },
        "CLIPBOARD" => if en {
            "The picture was saved, but the clipboard refused it."
        } else {
            "La photo est enregistrée, mais le presse-papiers l'a refusée."
        },
        "BUSY" => if en { "Finish the current capture first." } else { "Terminez d'abord la capture en cours." },
        "NO_PREVIEW" => if en { "The selection expired. Try again." } else { "La sélection a expiré. Recommencez." },
        "NO_MIC" => if en { "No microphone found. The film will not have that track." } else { "Aucun micro trouvé. Le film n'aura pas cette piste." },
        "NO_SYSTEM_AUDIO" => if en { "No system-audio source found." } else { "Aucune source pour le son du système." },
        "NO_CAMERA" => if en { "No camera found. The film will not have a bubble." } else { "Aucune caméra trouvée. Le film n'aura pas de bulle." },
        "DELETED" => if en { "Deleted." } else { "Supprimé." },
        "HOTKEY_OS" => if en { "The system refused a shortcut." } else { "Le système a refusé un raccourci." },
        "SCREEN_X11" => if en { "X11 session: no extra permission dialog." } else { "Session X11 : pas de fenêtre d'autorisation supplémentaire." },
        "SCREEN_WAYLAND" => if en { "Wayland: the desktop portal opens on the next screenshot." } else { "Wayland : le portail du bureau s'ouvre à la prochaine photo." },
        "SCREEN_MAC" => if en { "macOS will ask for screen recording access." } else { "macOS demandera l'accès à l'enregistrement de l'écran." },
        "SCREEN_WINDOWS" => if en { "Windows may ask the first time you capture." } else { "Windows peut demander une confirmation à la première capture." },
        _ => if en { "Something went wrong." } else { "Une erreur est survenue." },
    };
    msg.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn french_is_default_and_english_differs() {
        let fr = text("fr", "SHOT_SAVED");
        let en = text("en", "SHOT_SAVED");
        assert!(fr.contains("Photo"));
        assert!(en.contains("Picture"));
        assert_ne!(fr, en);
        assert!(text("fr", "WAYLAND_RECORDING").contains("Wayland"));
    }
}
