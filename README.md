<div align="center">

<img src="public/logo.png" alt="Anilili Logo" width="120" style="border-radius: 24px; box-shadow: 0 8px 24px rgba(137, 121, 242, 0.35);" />

# 🎬 Anilili Linux Desktop

### *Native, Ultra-Fast Anime Streaming & Library Management for Linux*

[![CI](https://github.com/MaidLover69/Anilili_Linux/actions/workflows/ci.yml/badge.svg)](https://github.com/MaidLover69/Anilili_Linux/actions)
[![Release](https://img.shields.io/badge/Release-v1.0.3-blue.svg)](https://github.com/MaidLover69/Anilili_Linux/releases)
[![Tauri v2](https://img.shields.io/badge/Tauri-v2.0-FFC131?logo=tauri&logoColor=white)](https://tauri.app)
[![Rust](https://img.shields.io/badge/Rust-2021-DEA584?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![React](https://img.shields.io/badge/React-18-61DAFB?logo=react&logoColor=black)](https://react.dev)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![Linux Platform](https://img.shields.io/badge/Platform-Linux%20(AppImage%20%7C%20RPM%20%7C%20DEB)-E95420?logo=linux&logoColor=white)](https://github.com/MaidLover69/Anilili_Linux/releases)

<p align="center">
  <b>Anilili Linux</b> is a lightweight, modern, native Linux anime streaming desktop application engineered with <b>Tauri v2</b>, <b>Pure Rust</b>, <b>React 18</b>, and <b>Tailwind CSS</b>. Designed for buttery-smooth 60fps desktop navigation, privacy, and zero telemetry.
</p>

</div>

> [!NOTE]
> **Disclaimer:** Anilili_Linux describes itself as a personal and educational project that is not affiliated with AniList, MyAnimeList, or streaming providers. It hosts no video content, and availability and legality can vary by region. Visitors remain responsible for the laws and terms that apply to them.

---

## 📑 Table of Contents

- [✨ Features](#-features)
  - [Cross-Anime Custom Playlists & Queues](#-cross-anime-custom-playlists--queues)
  - [Next-Gen In-App Player](#-next-gen-in-app-player)
  - [High-Throughput Provider Aggregator](#-high-throughput-provider-aggregator)
  - [Background Download Engine](#-background-download-engine)
  - [External Player Bridge (MPV / VLC)](#-external-player-bridge-mpv--vlc)
  - [AniList & MyAnimeList Synchronization](#-anilist--myanimelist-synchronization)
  - [Local Anime Storage Scanner](#-local-anime-storage-scanner)
- [📦 Installation & Packages](#-installation--packages)
  - [AppImage (Universal Linux)](#appimage-universal-linux)
  - [RPM (Fedora, RHEL, openSUSE)](#rpm-fedora-rhel-opensuse)
  - [DEB (Debian, Ubuntu, Linux Mint)](#deb-debian-ubuntu-linux-mint)
- [⌨️ Keyboard Shortcuts](#-keyboard-shortcuts)
- [🏗️ Architecture & Technology Stack](#-architecture--technology-stack)
- [🛠️ Building From Source](#-building-from-source)
- [⚙️ CI/CD & Automated Packaging](#-cicd--automated-packaging)
- [📄 Project Structure](#-project-structure)
- [⚖️ License](#-license)

---

## 📸 Screenshots

| Home | Anime page |
| :---: | :---: |
| ![Home](screenshots/home.png) | ![Anime page](screenshots/detail.png) |
| **Player** | **Discover** |
| ![Player](screenshots/player.png) | ![Discover](screenshots/discover.png) |
| **Schedule** | **Plugins** |
| ![Schedule](screenshots/schedule.png) | ![Plugins](screenshots/plugins-installed.png) |

---

## ✨ Features

### 📋 Cross-Anime Custom Playlists & Queues
- **Cross-Series Playlists**: Select individual episodes or multi-select entire arcs across different anime titles into custom playlists (e.g. *"Epic Tournament Fights"*, *"Shonen Weekend Marathon"*, *"Slice of Life Comfort"*).
- **Seamless Continuous Playback**: In-app player automatically advances from one anime title to the next, fetching stream sources dynamically.
- **Batch Downloading**: Queue every episode of a playlist for download; a background queue resolves sources and saves them with `ffmpeg`.
- **External Playlist Streaming**: Generate an `.m3u` playlist on the fly and open it in **mpv** or **VLC**, passing the referer the stream host expects.

### 🎥 Next-Gen In-App Player
- **HLS Streaming**: Powered by `hls.js` with adaptive bitrate and manual quality selection. Streams and subtitles are fetched through a local proxy in the Rust backend, which adds the `Referer`/`Origin` headers stream hosts require — so playback behaves the same in development and in packaged builds.
- **AniSkip Auto-Skip**: Detects and automatically skips (or offers a one-click floating pill for) opening (OP) and ending (ED) sequences.
- **Customizable Subtitles**: Adjust font size, text color, background and sync delay. WebVTT, SRT and ASS subtitles are supported (converted to WebVTT by the backend).
- **Audio & Subtitle Track Switching**: Effortlessly toggle between SUB (Japanese) and DUB (English) audio and multi-language soft subtitles.
- **Resume & Feedback**: Episodes resume where you left off; seek, skip and volume changes show brief on-player feedback, and loading or playback failures are shown with a retry.

### 🌐 Streaming Sources as Plugins
- **Nothing built in**: Anilili ships without any streaming source. Sources are JavaScript plugins the app downloads from a repository you add in **Settings → Plugins**; without one, anime and episode lists still show but nothing can be played.
- **Official plugin repository**: paste `https://github.com/MaidLover69/Anilili_Linux_plugins` to get started. Each plugin has its own on/off switch.
- **Seanime-compatible**: plugins use Seanime's `onlinestream-provider` format, so many community Seanime provider repositories can be added the same way.
- **Self-updating**: the app re-reads every added repository at launch and whenever you press **Refresh all**, so a broken source is fixed by updating its plugin, not the app.
- **Sandboxed**: plugins run in an embedded JavaScript engine with no file access; all their requests go through the app (DNS-over-HTTPS, stream proxy).
- **Automatic Fallback & Failover**: enabled plugins are tried at once and the first verified stream wins; when none works, the player lists the reason per plugin.
- **Sites behind a browser check**: when a site answers a plugin with an anti-bot page, the player offers "Verify <site>". That opens the site in an in-app browser window where you pass the check yourself; the app then asks that site through the same browser session. Nothing is solved or bypassed automatically.
- **DNS-over-HTTPS**: plugin requests can be resolved through Cloudflare, Google or AdGuard instead of your ISP's DNS (Settings → Network, on by default).

### ⚡ Background Download Engine
- **Powered by ffmpeg**: HLS streams are muxed with the system `ffmpeg`; direct files are downloaded by the backend itself.
- **Format Standardization**: HLS streams are remuxed into `.mkv` with subtitles embedded when available. Failed downloads show the reason and can be retried.
- **Pre-Flight Storage Verification**: Verifies disk space before initiating large series downloads.

### 🚀 External Player Bridge (MPV / VLC)
- **Native MPV & VLC Integration**: Prefer your system's standalone video player? Stream any episode or entire playlist with a single click.
- **Header Forwarding**: Passes the stream's `Referer` to the external player to avoid 403 hotlink blocks.

### 🔄 AniList & MyAnimeList Synchronization
- **AniList & MyAnimeList Tracking**: Sign in to either service in an in-app window. Finished episodes update your progress on the connected lists, adding to the watchlist adds the anime as planned, and list entries can be edited from the Library.
- **Airing Calendar**: Weekly schedule of broadcast releases with countdown timers, and a bell per anime for a desktop notification when a new episode airs (while the app is running).

### 📁 Local Anime Storage Scanner
- **Offline Indexer**: Scans local directories (e.g. `~/Videos/Anime`) and automatically parses filenames, series titles, and episode numbers.
- **Playback**: Open local files and finished downloads in mpv or VLC.

---

## 📦 Installation & Packages

Pre-compiled production packages are available on the [Releases](https://github.com/MaidLover69/Anilili_Linux/releases) page for every version.

### AppImage (Universal Linux)
Bundles its own media plugins; release builds are produced in CI on Ubuntu 22.04:
```bash
# Make executable
chmod +x Anilili_1.0.3_amd64.AppImage

# Launch Anilili
./Anilili_1.0.3_amd64.AppImage

# Note for Wayland users (Fedora/Niri/Sway/Hyprland) if EGL display error occurs:
LD_PRELOAD=/usr/lib64/libwayland-client.so ./Anilili_1.0.3_amd64.AppImage
```

### RPM (Fedora, RHEL, openSUSE)
Native RPM package registered with your desktop environment and system menu:
```bash
# Install via dnf
sudo dnf install ./Anilili-1.0.3-1.x86_64.rpm

# Or install via rpm
sudo rpm -i Anilili-1.0.3-1.x86_64.rpm
```

### DEB (Debian, Ubuntu, Linux Mint)
Native Debian package:
```bash
sudo apt install ./anilili_1.0.3_amd64.deb
```

---

## ⌨️ Keyboard Shortcuts

During in-app video playback:

| Key | Action |
| :--- | :--- |
| <kbd>Space</kbd> / <kbd>K</kbd> | Play / Pause |
| <kbd>F</kbd> | Toggle Fullscreen |
| <kbd>M</kbd> | Toggle Mute |
| <kbd>→</kbd> | Seek Forward 5 seconds (<kbd>Shift</kbd>: 30) |
| <kbd>←</kbd> | Seek Backward 5 seconds (<kbd>Shift</kbd>: 30) |
| <kbd>L</kbd> | Seek Forward 10 seconds |
| <kbd>J</kbd> | Seek Backward 10 seconds |
| <kbd>↑</kbd> | Volume Up (+5%) |
| <kbd>↓</kbd> | Volume Down (-5%) |
| <kbd>Shift</kbd> + <kbd>N</kbd> | Next Episode / Advance Playlist |
| <kbd>Shift</kbd> + <kbd>P</kbd> | Previous Episode |
| <kbd>S</kbd> | Skip Intro / Outro (while the skip button is shown) |
| <kbd>Esc</kbd> | Exit Fullscreen / Close Dialogs |

Anywhere in the app: <kbd>Ctrl</kbd> + <kbd>K</kbd> opens search, <kbd>?</kbd> shows this list.

---

## 🏗️ Architecture & Technology Stack

```
┌────────────────────────────────────────────────────────┐
│                   React 18 Frontend                    │
│   TypeScript • Tailwind CSS • TanStack Query • Zustand │
├────────────────────────────────────────────────────────┤
│                   Tauri v2 IPC Core                    │
│           WebKitGTK Webview (Hardware DMA-BUF)         │
├────────────────────────────────────────────────────────┤
│                   Rust Backend Core                    │
│  tokio • reqwest • sqlx (SQLite) • regex • tiny_http   │
├──────────────────┬──────────────────┬──────────────────┤
│ Stream Providers │ Cloud Providers  │ External Players │
│   JavaScript     │  AniList, MAL,   │    MPV, VLC,     │
│    plugins       │    AnimeThemes   │      ffmpeg      │
└──────────────────┴──────────────────┴──────────────────┘
```

- **Frontend**: React 18, TypeScript, Tailwind CSS, Vite, TanStack Query v5, Zustand, Lucide React.
- **Backend**: Rust 2021 edition, Tauri v2, Tokio asynchronous runtime, SQLx SQLite, Reqwest, and a loopback stream proxy (`tiny_http`).
- **Desktop Packaging**: Native `.AppImage`, `.rpm`, and `.deb` bundles generated via `@tauri-apps/cli`.

---

## 🛠️ Building From Source

### Prerequisites

#### 1. System Dependencies

**Fedora / RHEL:**
```bash
sudo dnf install -y \
  webkit2gtk4.1-devel \
  openssl-devel \
  curl \
  wget \
  file \
  libappindicator-gtk3-devel \
  librsvg2-devel \
  gstreamer1-plugins-good \
  gstreamer1-plugins-bad-free \
  ffmpeg \
  mpv
```

**Ubuntu / Debian:**
```bash
sudo apt update && sudo apt install -y \
  libwebkit2gtk-4.1-dev \
  build-essential \
  curl \
  wget \
  file \
  libssl-dev \
  libgtk-3-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  gstreamer1.0-plugins-good \
  gstreamer1.0-plugins-bad \
  gstreamer1.0-libav \
  ffmpeg \
  mpv
```

`ffmpeg` is needed for downloads and `mpv` (or VLC) for external playback; the in-app player needs the GStreamer plugins.

#### 2. Node.js & Rust Toolchains
- [Node.js 18+](https://nodejs.org) (`node -v`)
- [Rust & Cargo](https://rustup.rs) (`rustc --version`)

### Compilation Steps

```bash
# 1. Clone the repository
git clone https://github.com/MaidLover69/Anilili_Linux.git
cd Anilili_Linux

# 2. Install frontend dependencies
npm install

# 3. Launch live development mode with hot-reloading
npm run tauri dev

# 4. Compile optimized release binaries and packaging bundles
npx @tauri-apps/cli build
```

Build from a directory whose path has no spaces: the AppImage tooling writes the project path into
its launcher script, and a space breaks the bundled TLS module lookup. An AppImage built on a
rolling distribution only runs reliably on similar systems — use the CI release workflow for
packages you distribute.

Generated packages will be in:
`src-tauri/target/release/bundle/` (`appimage/`, `rpm/`, `deb/`).

---

## ⚙️ CI/CD & Automated Packaging

The repository includes GitHub Actions workflows under `.github/workflows/`:
- **`ci.yml`**: Runs on every pull request and push to `main`. Executes TypeScript validation, Vite production build, Rust `cargo check`, and the complete test suite.
- **`release.yml`**: Triggers on tag pushes (e.g. `v1.0.3`) or manual workflow dispatch. Compiles production packages across `.AppImage`, `.rpm`, and `.deb` and publishes them to GitHub Releases.

---

## 📄 Project Structure

```
Anilili_Linux/
├── .github/
│   └── workflows/
│       ├── ci.yml               # Automated CI build and testing
│       └── release.yml          # Multi-target release packaging (RPM, DEB, AppImage)
├── src/                         # React Frontend Application
│   ├── components/              # Modular UI components (Player, Playlist, EpisodeBrowser)
│   ├── hooks/                   # Custom TanStack Query & IPC hooks
│   ├── screens/                 # Top-level view screens (Home, Detail, Watch, Library)
│   ├── stores/                  # Zustand client stores (PlayerStore, UIStore)
│   ├── lib/                     # Stream proxy URL helpers
│   ├── theme/                   # Global styles and design tokens (CSS variables)
│   ├── types/                   # TypeScript interfaces and schema types
│   ├── App.tsx                  # Root app layout
│   └── router.tsx               # Hash-based desktop router
├── src-tauri/                   # Tauri v2 Rust Backend
│   ├── src/
│   │   ├── commands/            # Tauri IPC command handlers (Playlists, Streams, Sync)
│   │   ├── downloads/           # Download queue worker and ffmpeg muxing
│   │   ├── models/              # Serde data structs & SQLite entities
│   │   ├── plugins/             # JavaScript plugin runtime, repositories and registry
│   │   ├── providers/           # Provider manager: races plugins, verifies streams
│   │   ├── db.rs                # SQLite migrations & queries
│   │   ├── stream_proxy.rs      # Loopback proxy that adds stream headers and rewrites HLS playlists
│   │   └── lib.rs               # Application lifecycle & command registration
│   ├── tests/                   # Rust backend unit & integration tests
│   └── tauri.conf.json          # Tauri application and packaging manifest
├── index.html                   # Desktop webview shell host template
├── package.json                 # Node dependencies and scripts
└── vite.config.ts               # Vite configuration with desktop asset path resolution
```

---

## 🤝 Contributing

Contributions are warmly welcome! Whether you are fixing a bug, adding support for a new streaming provider, improving Linux desktop integration, or refining the UI/UX, your help makes Anilili Linux better for everyone.

### How to Contribute
1. **Fork the Repository** on GitHub.
2. **Create a Feature Branch**:
   ```bash
   git checkout -b feature/amazing-feature
   ```
3. **Make Your Changes**:
   - Adhere to the existing TypeScript and Rust coding conventions.
   - Keep pull requests focused on a single change or feature.
4. **Run Verification & Tests**:
   ```bash
   # Validate frontend build & types
   npm run build

   # Run Rust cargo check and tests
   cargo test --manifest-path src-tauri/Cargo.toml
   ```
5. **Commit Your Changes**:
   ```bash
   git commit -m "feat: add amazing feature"
   ```
6. **Push to Your Branch**:
   ```bash
   git push origin feature/amazing-feature
   ```
7. **Open a Pull Request** describing your changes, motivation, and test results.

---

## 🐛 Bug Reports & Feature Requests

Have you encountered an issue or have a great idea to make Anilili Linux even better? We'd love to hear from you!

- **Bug Reports**: If you experience a crash, playback failure, UI glitch, or broken provider, please [open an issue](https://github.com/MaidLover69/Anilili_Linux/issues) with:
  - Your Linux distribution and desktop environment (e.g. Fedora 40 GNOME, Arch KDE).
  - Package format used (`.AppImage`, `.rpm`, `.deb`, or built from source).
  - Steps to reproduce the problem and the backend log (`~/.local/share/anilili/anilili.log` in packaged builds).
- **Feature Requests**: Have ideas for new player controls, tracker integrations, playlist enhancements, or provider sources? Feel free to [submit a feature request](https://github.com/MaidLover69/Anilili_Linux/issues) detailing the use case and expected experience.

---

## 💖 Acknowledgments & Inspirations

Anilili Linux stands on the shoulders of the vibrant open-source anime and media software community. Special thanks and heartfelt appreciation to:

- **[Anilili Android](https://github.com/kompoti121/anilili)**: The original Android application that started it all, providing the foundational inspiration, UX vision, and feature set that motivated this native Linux desktop port.
- **[Aniyomi](https://github.com/aniyomiorg/aniyomi) & [Dantotsu](https://github.com/rebelonion/Dantotsu)**: For pioneering mobile anime extensions, robust scrapers, and unified tracker integration patterns.
- **[Seanime](https://github.com/5rahim/seanime)**: For proving how elegant, rich, and responsive modern desktop anime media management can be.
- **[LiSA](https://github.com/LiSA-org)**: For creative inspiration in lightweight desktop anime streaming and sleek UI layouts.
- **[Miru](https://github.com/ThaUnknown/miru)**: For pushing the boundaries of modern desktop anime streaming interfaces and multi-platform experiences.
- **[Tauri](https://tauri.app)** & **[Vite](https://vitejs.dev)**: For enabling the creation of ultra-fast, memory-efficient, and secure native desktop applications without the heavy overhead of Chromium.

---

## ⚖️ License

Distributed under the **MIT License**. See [`LICENSE`](LICENSE) for more details.
