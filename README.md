# Lueur

Lueur capture l'écran et enregistre un film, en local, sur Windows, macOS et Linux. L'interface est native, dessinée par [GPUI](https://gpui.rs), le cadre GPU de Zed — celui que Cap a choisi en quittant le navigateur. Il n'y a pas de webview.

Le moteur est séparé de l'interface : `xcap` parle aux API natives (X11, portail Wayland, ScreenCaptureKit, Windows Graphics) et FFmpeg encode le MP4.

## Cette version

- Photo de l'écran, d'une fenêtre ou d'une zone. PNG, presse-papiers et dossier.
- Film de l'écran, d'une fenêtre ou d'une zone. Pause, reprise, stop. MP4 local. Micro, son du système et bulle caméra quand le matériel existe.
- Icône native dans la barre de statut.
- Raccourcis globaux.
- Bibliothèque : ouvrir, montrer le dossier, supprimer.
- Réglages : dossier, hauteur jusqu'à la 4K sans agrandir, cadence, qualité, curseur, micro, son, bulle.
- Français par défaut, anglais au bouton de langue.

## Plus tard

- Montage : couper, zoomer sur les clics.
- Liens de partage avec un serveur que l'on héberge soi-même.
- Commentaires.
- Habillage de marque.

Les fichiers restent des PNG et des MP4. `library.json` décrit leur source pour un futur monteur.

## Lancer

Rust stable, FFmpeg dans le `PATH`, et Vulkan (Mesa ou lavapipe sur Linux).

```bash
# Linux
sudo apt install libvulkan1 mesa-vulkan-drivers libxkbcommon-dev libxkbcommon-x11-dev \
  libwayland-dev libx11-dev libxcb1-dev libxcb-randr0-dev libxcb-shm0-dev \
  libdbus-1-dev pkg-config libclang-dev libpipewire-0.3-dev libgtk-3-dev ffmpeg
cargo run
```

```bash
# macOS
xcode-select --install
brew install ffmpeg
cargo run
```

```bash
# Windows : FFmpeg dans le PATH, puis
cargo run
```

La première capture macOS demande l'écran, le micro et la caméra dans Réglages Système. Le son du système y exige une boucle (BlackHole, Loopback). Sous Windows, « Stereo Mix » sert de son système. En Wayland pur, les photos passent par le portail ; le film continu n'est pas encore relié à FFmpeg et Lueur le dit. Une session X11 ou XWayland filme.

## Raccourcis

| Action | Raccourci |
| --- | --- |
| Photo de l'écran | Ctrl+Maj+1 |
| Photo d'une fenêtre | Ctrl+Maj+2 |
| Zone | Ctrl+Maj+3 |
| Film / stop | Ctrl+Maj+4 |
| Pause | Ctrl+Maj+5 |

Sur macOS, Ctrl est ⌘.

## Tests

```bash
cargo test
```

Géométrie, réglages, arguments FFmpeg des trois OS, bibliothèque et parseurs audio. Avec `DISPLAY`, une photo réelle et un MP4 d'une seconde sont aussi produits.

## Limites

- Vérifié sur Linux X11 : fenêtre GPUI, photo PNG 1920×1200, film MP4. Windows et macOS sont compilés par l'intégration continue, pas exécutés ici.
- Un film plein écran inclut la fenêtre de Lueur.
- Une fenêtre est filmée selon le rectangle qu'elle occupe au démarrage.
- Sans PulseAudio, micro ni caméra, le film est silencieux et sans bulle.
- GPUI a besoin de Vulkan. `lavapipe` sert de repli logiciel.

## Licence

Apache 2.0. Utilisation, modification et redistribution sont permises. Toute redistribution conserve le crédit de l'auteur : le copyright Karom Studio et le fichier [NOTICE](NOTICE). Les fichiers modifiés indiquent qu'ils ont été modifiés. Voir [LICENSE](LICENSE).
