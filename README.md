# Kapture Desktop

Kapture — An open-source project by Karom Studio.

Kapture Desktop est un logiciel open source pour PC : Windows, macOS et Linux. Il capture l'écran et enregistre un film, en local. L'interface est native, dessinée par [GPUI](https://gpui.rs), le cadre GPU de Zed.

| | |
| --- | --- |
| Organisation | Karom Studio |
| Dépôt | [github.com/Karom-Studio/kapture](https://github.com/Karom-Studio/kapture) |
| Application | Kapture Desktop |
| Documentation | [kapture.karom.studio](https://kapture.karom.studio) |
| Paquet npm éventuel | `@karom-studio/kapture` |

Le binaire s'appelle `kapture`. Les fichiers portent le préfixe `Kapture_`. Le paquet npm n'est pas publié : l'application est un programme Rust, sans dépendance JavaScript.

Le moteur est séparé de l'interface : `xcap` parle aux API natives (X11, portail Wayland, ScreenCaptureKit, Windows Graphics) et FFmpeg encode le MP4.

## Cette version

- Photo de l'écran, d'une fenêtre ou d'une zone. La fenêtre de Kapture est masquée le temps de la prise. PNG, presse-papiers et dossier. Éditer ajoute un trait, une flèche, un cadre, une coche, une croix, une étoile, du texte, ou un recadrage.
- Film de l'écran, d'une fenêtre ou d'une zone. Pause, reprise, stop. MP4 local. Éditer coupe le début et la fin, incruste une ligne de texte, et prend une photo de l'instant choisi. Micro et son du système quand le matériel existe. La webcam en incrustation se choisit avant l'enregistrement : sans, ou petite, moyenne et grande, dans un coin.
- Icône native dans la barre de statut.
- Raccourcis globaux.
- Bibliothèque : mosaïque avec les vignettes, ou liste avec l'aperçu au survol. Une photo s'ouvre dans Kapture. Un film s'ouvre dans un lecteur vidéo.
- Envoi direct vers une autre machine Kapture, ou vers un serveur que l'on héberge soi-même.
- Réglages : dossier, hauteur jusqu'à la 4K sans agrandir, cadence, qualité, curseur, micro, son, presets webcam, adresse et jeton de partage.
- Français par défaut, anglais au bouton de langue.

## Plus tard

- Floutage.
- Export JPEG et WebP, en plus du PNG.
- Montage sur une timeline : couper au milieu, zoomer sur les clics.

Les fichiers restent sur le disque. `library.json` décrit leur source pour ces étapes.

## Partage

Le serveur héberge la capture et renvoie un lien. La personne peut l'ouvrir plus tard, sans être connectée au moment de l'envoi.

```bash
kapture serve --bind 0.0.0.0:8741 --token un-jeton-long
```

Dans Réglages, mode **Serveur**, adresse `http://votre-machine:8741`, même jeton. **Envoyer** dans la bibliothèque dépose le fichier et copie le lien. La page d'accueil du serveur liste les captures.

Le mode **Direct** envoie vers un autre Kapture allumé à ce moment-là. Si ce Kapture est éteint, l'envoi échoue : c'est pour ça que le lien passe par le serveur.

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

La première capture macOS demande l'écran, le micro et la caméra dans Réglages Système. Le son du système y exige une boucle (BlackHole, Loopback). Sous Windows, « Stereo Mix » sert de son système. En Wayland pur, les photos passent par le portail ; le film continu n'est pas encore relié à FFmpeg et Kapture Desktop le dit. Une session X11 ou XWayland filme.

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
- Un film plein écran inclut la fenêtre de Kapture Desktop.
- Une fenêtre est filmée selon le rectangle qu'elle occupe au démarrage.
- Sans PulseAudio, micro ni caméra, le film est silencieux et sans bulle.
- GPUI a besoin de Vulkan. `lavapipe` sert de repli logiciel.

## Licence

Apache 2.0. Utilisation, modification et redistribution sont permises. Toute redistribution conserve le crédit de l'auteur : le copyright Karom Studio et le fichier [NOTICE](NOTICE). Les fichiers modifiés indiquent qu'ils ont été modifiés. Voir [LICENSE](LICENSE).
