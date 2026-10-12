# Changelog

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
  Manga and Comic.

## 1.0.3

### Plugins
- Sites behind a browser check: when a site answers a plugin with an anti-bot page, the player
  offers "Verify <plugin> (<site>)". The site opens in an in-app browser window, you pass the
  check yourself, and the app then sends that plugin's requests to the site through the same
  browser session (GET and POST, binary-safe). Verified sites are remembered across restarts.
  Nothing is solved or bypassed automatically.
- Seanime compatibility: `{{field}}` placeholders from a plugin's `userConfig` are filled with
  the saved value or the default, and `$scannerUtils` is available, so more community plugins
  load.
- A stream that answers with a web page or an anti-bot page instead of video is rejected with
  that reason instead of being offered.

### Player
- Direct MP4 streams now report their length and accept seeking through the stream proxy.
  Files with their index at the end used to fail with "The video could not be loaded".

## 1.0.2

### Streaming sources are plugins
- The app no longer contains any streaming source. Sources are JavaScript plugins in Seanime's
  `onlinestream-provider` format, run in a sandboxed engine and installed from a repository
  link in **Settings → Plugins**, each with its own on/off switch.
- Added repositories are re-read at every launch and with **Refresh all**.
- With no plugin enabled the app still shows anime and episodes and says what is missing.

### Playback
- Streams and subtitles are fetched through a local proxy that adds the headers stream hosts
  require, so packaged builds play the same as development builds.
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
