<div align="center">

<img src="public/logo.png" alt="Anix logo" width="120" />

# Anix

### An anime catalogue, tracker, and local library player for Linux

[![Platform](https://img.shields.io/badge/Platform-Linux%20(AppImage%20%7C%20RPM%20%7C%20DEB)-E95420?logo=linux&logoColor=white)](https://anix-linux.vercel.app)
[![Version](https://img.shields.io/badge/Version-1.1.0-blue.svg)](CHANGELOG.md)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)

<p align="center">
  <b>Anix</b> is a native Linux desktop application for cataloguing and tracking anime. Browse the AniList catalogue, keep your AniList and MyAnimeList watchlists in sync, follow weekly airing schedules with desktop notifications, and organize and play your local anime media library using the built-in player or MPV / VLC. Zero telemetry.
</p>

**Website:** <https://anix-linux.vercel.app>

</div>

---

## Contents

- [Screenshots](#screenshots)
- [Features](#features)
- [Installation](#installation)
- [Extensions](#extensions)
- [Keyboard Shortcuts](#keyboard-shortcuts)
- [Built With](#built-with)
- [License](#license)

---

## Screenshots

| Home | Anime Page |
| :---: | :---: |
| ![Home](screenshots/home.png) | ![Anime page](screenshots/detail.png) |
| **Player** | **Discover** |
| ![Player](screenshots/player.png) | ![Discover](screenshots/discover.png) |
| **Schedule** | **Library** |
| ![Schedule](screenshots/schedule.png) | ![Extensions](screenshots/plugins-installed.png) |

---

## Features

### Catalogue & Discovery
- **AniList Catalogue**: Comprehensive home, discover, and search views with detailed metadata, episode lists, character information, and recommendations.
- **Theme Songs & Metadata**: Opening and ending theme listings and episode markers.

### Tracker & Progress Sync
- **AniList & MyAnimeList**: Connect your AniList or MyAnimeList accounts. Finished episodes update your watch progress automatically, and watchlist status is synchronized seamlessly.
- **Watch History**: Local per-episode watch history with resume-playback support.

### Local Media Library
- **Local Folder Scanner**: Scan your local media directories (e.g. `~/Videos/Anime`). Anix parses filenames, maps series titles and episode numbers, and tracks watch progress for your local files.
- **Player Integration**: Play your local media with the integrated player or hand off playback to external players like **mpv** or **VLC**.
- **Playlists & Queues**: Create custom playlists, continuous playback, and export `.m3u` playlists for external media players.

### Airing Schedule & Notifications
- **Weekly Schedule**: Visual weekly calendar of upcoming broadcasts with countdown timers.
- **Airing Alerts**: Optional desktop notifications when new episodes air.

### Video Player
- **Integrated Player**: Hardware-accelerated video playback with subtitle customisation (ASS, SRT, WebVTT font, size, and timing delay).
- **AniSkip**: Automatic or one-click skip for opening and ending sequences.
- **External Player Support**: Instant one-click launch with **mpv** or **VLC**.

### Interface & Performance
- **Built for Linux**: Lightweight native application powered by Tauri v2 and Rust. Low memory footprint and snappy performance.
- **Customizable Themes**: Dark, Light, Mocha, Hacker, Dracula, Tokyo Night, Manga, and Comic themes.
- **Keyboard-Driven**: Quick search with <kbd>Ctrl</kbd> + <kbd>K</kbd> and comprehensive in-player hotkeys.
- **Privacy First**: No analytics or telemetry.

---

## Extensions

Anix features a modular plugin architecture supporting JavaScript extensions for custom community providers and metadata resolvers.

- **Sandboxed Execution**: Extensions execute within an isolated QuickJS JavaScript engine without direct access to system files or OS processes.
- **Seanime Provider Compatibility**: Extensions follow the standard `onlinestream-provider` format.
- **Easy Management**: Add, enable, or configure provider links directly in **Settings → Plugins**.

---

## Installation

Packages for Linux are available from the project website: <https://anix-linux.vercel.app/download> and [Codeberg Releases](https://codeberg.org/MaidLover69/Anix/releases).

- **Direct AppImage Download:** [Anix_1.1.0_amd64.AppImage](https://drive.google.com/file/d/1c5IhzjZMVEbJJ6K3B1bZxlU4nWjh8ChI/view?usp=sharing)

### AppImage (Universal, any distribution)

```bash
chmod +x Anix_1.1.0_amd64.AppImage
./Anix_1.1.0_amd64.AppImage
```

*(Wayland note: On certain Wayland compositors, if an EGL display error occurs, launch with: `LD_PRELOAD=/usr/lib64/libwayland-client.so ./Anix_1.1.0_amd64.AppImage`)*

### RPM (Fedora, openSUSE, RHEL)

```bash
sudo dnf install ./Anix-1.1.0-1.x86_64.rpm
```

### DEB (Ubuntu, Debian, Linux Mint)

```bash
sudo apt install ./Anix_1.1.0_amd64.deb
```

---

## Where Anix Stores Data

| Item | Location |
| :--- | :--- |
| Database, watch history, settings | `~/.local/share/anix/` |
| Configuration | `~/.config/anix/settings.json` |
| Image cache | `~/.cache/anix/` |

---

## Keyboard Shortcuts

During playback:

| Key | Action |
| :--- | :--- |
| <kbd>Space</kbd> / <kbd>K</kbd> | Play / Pause |
| <kbd>F</kbd> | Toggle fullscreen |
| <kbd>M</kbd> | Toggle mute |
| <kbd>→</kbd> / <kbd>←</kbd> | Seek 5 seconds (<kbd>Shift</kbd>: 30s) |
| <kbd>L</kbd> / <kbd>J</kbd> | Seek 10 seconds |
| <kbd>↑</kbd> / <kbd>↓</kbd> | Volume up / down (5%) |
| <kbd>Shift</kbd> + <kbd>N</kbd> / <kbd>P</kbd> | Next / Previous episode |
| <kbd>S</kbd> | Skip intro / outro |
| <kbd>Esc</kbd> | Exit fullscreen |

Anywhere in the app: <kbd>Ctrl</kbd> + <kbd>K</kbd> opens search, <kbd>?</kbd> shows keyboard shortcuts.

---

## Built With

- **Frontend**: React 18, TypeScript, Tailwind CSS, Vite, TanStack Query, Zustand, Lucide.
- **Backend**: Rust, Tauri v2, Tokio, WebKitGTK, QuickJS sandbox.

---

## License

Distributed under the **MIT License**. See [LICENSE](LICENSE) for details.
