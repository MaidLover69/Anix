# Changelog

## 1.3.1

### Languages
- Italian interface: the app, its notifications, tray menu and reminders are now available in Italian (Settings › Appearance › Language, or the first-start setup). Like the other languages, it was translated by AI.

### Application Updates
- Automatic update notification: when a newer release is detected, the update window opens automatically once after start-up (safely scheduled so it never interrupts the video player or appears over other active dialogs).

### Downloads & Offline Media
- Direct disk deletion on download removal: deleting a completed download now offers an option (enabled by default) to remove the video file and its companion subtitle files (`.vtt`, `.ass`, `.srt`) directly from disk.
- Safe path validation: strict checks ensure only downloaded video files and their associated subtitle side files within the download directory can be deleted.

### Video Player & System Integration
- Crash on click fixed: with the embedded mpv player on, a left click in the window could close Anix at once (the window's resize handling expected a different widget layout). The mpv layer now sits where it expects.
- Focused window verification: the "Is the video player showing correctly?" countdown only elapses while the application window is visible, unminimized, and actively focused, preventing unexpected fallbacks to the web player when multitasking across other windows or workspaces.
- In-app notification on fallback: if the player confirmation prompt expires without user interaction, a notice is added to the Notification Centre explaining the switch and indicating where to re-enable the embedded mpv engine (Settings › Playback › Player engine).
- Host environment sanitization: external links, system browser OAuth sign-ins (such as Comick), and host media players now launch with sanitized environment variables, preventing AppImage bundle library paths (`GTK_PATH`, `GIO_EXTRA_MODULES`, etc.) from interfering with system applications.

### Catalogue & Performance
- Resilient episode metadata fetching: reduced MyAnimeList/Jikan request timeout from 10s to 5s, with an automatic temporary cooldown when the service is unreachable or timing out, preventing catalogue pages from stalling.
- Diagnostics logging: OAuth sign-in windows log sanitized page URLs (host and path only, strictly omitting query parameters and sensitive credentials) to assist with troubleshooting.

### Documentation & Packaging
- Packaging documentation: updated README with distribution-specific FUSE 2 package requirements across Arch, Debian/Ubuntu, openSUSE, and Fedora, along with `--appimage-extract-and-run` guidance.
- AppImage build script: anchored the `/dist` exclusion rule in `scripts/build-appimage.sh` to prevent accidentally excluding npm package distribution directories.
- Wider distribution support: the release AppImage is now built on Ubuntu 22.04 by `.github/workflows/release.yml`, so it runs on Ubuntu 22.04 and 24.04, Linux Mint 21+, Debian 12+ and every newer distribution (1.3.0, built on Fedora 44, needed the very newest system libraries). The web player's GStreamer decoding uses a matching LGPL FFmpeg build on older bases.

## 1.3.0

### Languages and First-Start Setup
- Multilingual interface support: English, French, Japanese, Korean, Simplified Chinese, Brazilian Portuguese, Russian, Hindi, Spanish, German, and Arabic (with complete right-to-left layout). Configurable in Settings › Appearance › Language, with automatic system language detection.
- Streamlined setup wizard on first launch and following updates: easily configure preferred display language along with first, second, and third priority preferences for audio dubs and subtitles.
- Desktop notifications, release alerts, airing countdowns, and system tray menu automatically follow the selected interface language.

### Video Player & Playback Engine
- Embedded libmpv is now the primary player engine on Linux, rendered directly within the application window with custom controls. The web player remains available as a fallback (Settings › Playback › Player engine) and automatically activates if libmpv cannot be initialized.
- Automatic hardware video decoding with safe defaults and automated graphics self-tests.
- Anime4K real-time upscaling shader presets (Off, Mode A, B, C; Fast or High Quality).
- Configurable playback buffer and disk cache settings, aspect ratio modes (Fit, Crop, Fill), and timestamp clock displaying estimated completion time.
- Configurable intro and outro skip toggles with a 3-2-1 visual countdown (Enter to skip, Esc to cancel) and manual +85s skip option when segment metadata is unavailable.
- Hold Space for 2× playback speed, keyboard step adjustments (`[` / `]`), and an in-player shortcut cheat sheet.
- Subtitle rendering positioned above the bottom control bar, with automatic audio track matching for multi-language streams.

### Release Alerts & Airing Reminders
- Airing alerts monitor availability for newly aired episodes (checking sub and dub tracks separately every 15 minutes for up to 3 hours) and send desktop notifications when ready.
- Background release monitoring when the application is closed (via system startup option).
- Episode airing reminders with customizable countdown intervals (10, 30, or 60 minutes) per series.

### Library & Watchlist Management
- Comprehensive list statuses (Watching, Plan to watch, Completed, On hold, Dropped) with real-time counters and filter views. Synchronizes bidirectionally with AniList, MyAnimeList, and Comick, with local offline fallback.
- Poster context menu (Right-click or Shift+F10) for rapid status updates, details inspection, and sharing.
- Encrypted backup and restore for library collections and custom settings.
- Notification Centre with dedicated Airing and Media tabs and unread badge indicator.

### Schedule & Discovery
- Schedule timeline featuring a dynamic current time marker, episode countdown timers, Up Next showcase, and a month calendar on wide displays.
- Series watch order visualizer, "Because you watched..." recommendations, advanced Discover filters (genre inclusion/exclusion, score threshold, release year, season), and recent search history in Command Palette (<kbd>Ctrl</kbd> + <kbd>K</kbd>).
- Reorderable home screen sections (Settings › Appearance).

### Offline Media & Downloads
- Dedicated Downloads management screen, custom storage folder picker, disk capacity meter, language tagging, automated transfer resumption on launch, and status notifications.

### Modular Extension System
- Per-extension custom configuration panels, automated background health checks, and changelog visibility for extension updates.
- Detailed, user-friendly diagnostic messages on stream playback issues, language badges across all provider listings, and draggable stream server prioritization.
- Built-in DNS-over-HTTPS fallback for enhanced privacy and network reliability, resilient connection timeouts, and automatic retry handling.

### Settings & User Interface
- Searchable settings with a two-pane layout on wide displays.
- Custom accent colors, contrast adjustment, interface scale slider, 12/24-hour clock formatting, and dynamic background art.
- Two-column detail page layout for resolutions above 1200px, refined episode cards, and intuitive empty states.
- Diagnostic error reporting with quick-copy error codes and system diagnostics export.
- Integrated update verification for AppImage packages.

## 1.2.0

### Languages & Audio Preferences
- Choose your subtitle and dub languages in order of preference (Settings › Sources). Automatically prioritizes providers in those languages.
- Server and stream selections are organized by language; the player indicates which language is active, defaults to your preferred subtitle track, and selects your preferred audio language for multi-audio streams.

### Local Media & Library Linking
- "Load your own episodes" on an anime's page links a folder or local files to that title. Linked episodes show "Local", play in the built-in player with resume, skip times, and AniList/MAL sync, and automatically detect companion subtitle files.
- Library › Local files recognizes release filenames, groups them into series, matches them to AniList metadata with posters, and links them in one click.

### Video Player
- Enhanced playback buffering: at higher playback speeds, video buffers smoothly during network stalls instead of frequent interruptions.
- Subtitle passthrough for mpv and VLC: external players now receive the active subtitles. mpv resumes from your current timestamp, with progress saved and synced back to your connected trackers.
- Desktop MPRIS media controls and hardware media key support.
- Compact mini-player mode.
- Dialogue boost audio processing for clearer speech.
- Subtitle drag-and-drop directly onto the player window.
- Secondary subtitle track support (displayed at the top of the video) and adjustable subtitle positioning.

### Trackers
- Comick integration as an additional tracker.

### Downloads & Offline Media
- Real-time download progress tracking and graceful background process management upon app exit.

### Interface & Customization
- Dynamic ambient glow from cover art.
- Quick hover peek previews for anime titles.
- Sticky action bar on anime detail pages.
- Native desktop window control styles.
- Pure OLED Black theme and system accent color matching.
- Schedule filtering: "Only my watchlist" toggle.
- Discover view: search history and "Surprise me" random title selector.
- Episode bulk actions: mark 1–N as watched.
- Library: dedicated Stats dashboard tab.
- Layout memory for episode and schedule views.

### System & Performance
- Optional system tray mode with background airing notifications when the main window is closed.
- Launch on desktop startup option.
- Offline mode: cached home, anime details, search results, and schedule display when network or AniList is unreachable.
- Hardware video decoding check in Settings › System.
- Extension management: health check testing, update checking, and curated provider lists.

## 1.1.0

### Anilili is now Anix
- New name, window title, desktop entry and package names (`Anix_1.1.0_amd64.AppImage`,
  `Anix-1.1.0-1.x86_64.rpm`, `Anix_1.1.0_amd64.deb`). Links use `anix://`; `anilili://` links
  still open the app.
- Your data moves with it. The first time Anix starts it finds an Anilili install and renames
  its folders: `~/.local/share/anilili` → `~/.local/share/anix` (database, downloads,
  extensions, log), `~/.config/anilili` → `~/.config/anix` (settings), `~/.cache/anilili` →
  `~/.cache/anix`, and `~/.local/share/io.anilili.linux` → `~/.local/share/io.anix.app` (player
  preferences and site sessions). History, watchlist, playlists, downloads, settings,
  installed extensions and sign-ins carry over. Nothing that already exists under the new
  names is overwritten, what was done is written to the log, and the app starts even when
  something could not be moved. Close Anilili before the first start of Anix.
- Extensions written for Anilili run unchanged: the `$anilili` helpers are also available as
  `$anix`.

### Updates
- The update check now reads the latest version from the project website instead of GitHub,
  and the download button opens the website's download page.

### Extensions
- An extension list can be hosted on Google Drive. Share links (`…/file/d/<id>/view`,
  `…/open?id=<id>`, `…/uc?id=<id>`) are accepted, and so is a web address that redirects to
  one; the shared file itself is downloaded.
- When Drive answers with a page instead of the file (sign-in, download quota or virus-scan
  notice) the app says "Google Drive did not return the file" with the reason, and the
  extensions already installed stay as they are.
- A list can name itself: `{ "name": "…", "plugins": [ … ] }` is accepted next to a bare array.
- Code that comes inline in a list is replaced whenever it changes, even when the extension's
  version number stays the same.

### Themes
- A theme selector in Settings: Default dark, Light, Mocha, Hacker, Dracula, Tokyo Night,
  Manga, Comic, and OLED Black.

## 1.0.3

### Plugins
- Verification helper: when a provider requires human interaction or session verification, an in-app window allows completing the verification and maintaining the active session.
- Seanime compatibility: `{{field}}` placeholders from a plugin's `userConfig` are filled with
  the saved value or the default, and `$scannerUtils` is available, so more community plugins
  load.
- Unreachable or invalid video endpoints are cleanly handled with specific error statuses.

### Player
- Direct MP4 streams now report their length and accept seeking through the stream proxy.

## 1.0.2

### Streaming sources are plugins
- The app no longer contains any streaming source. Sources are JavaScript plugins in Seanime's
  `onlinestream-provider` format, run in a sandboxed engine and installed from a repository
  link in **Settings → Plugins**, each with its own on/off switch.
- Added repositories are re-read at every launch and with **Refresh all**.
- With no plugin enabled the app still shows anime and episodes and says what is missing.

### Playback
- Streams and subtitles are fetched through a local proxy that adds required headers.
- Rebuilt player: loading and error states, fallback to the next stream, resume, working
  server, subtitle, audio and quality selection, subtitle delay, SRT and ASS subtitles.
- AppImage builds bundle the media plugins they need.

### Library, accounts and notifications
- Watch history is kept per episode (existing history is migrated).
- AniList and MyAnimeList sign-in in an in-app window; finished episodes update the connected
  lists; list entries can be edited from the Library. Account tokens stay in the backend.
- A bell per anime sends a desktop notification when a new episode airs while the app runs.
- Downloads have a real queue, report why they failed and can be retried.

### Network and security
- DNS-over-HTTPS (Cloudflare, Google or AdGuard) for all backend requests.
- External players are limited to mpv and VLC and only receive http(s) URLs; the settings
  file is written atomically and readable by the owner only; a content security policy is set.

### Interface
- Repaired theme tokens, shared buttons, dialogs, tabs and switches, calmer typography,
  bundled fonts, keyboard focus and screen-reader labels.

### Removed
- The built-in providers, the legacy Python application and Discord presence.

## 1.0.1

- Initial Tauri release.
