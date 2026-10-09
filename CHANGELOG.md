# Changelog

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
  Files with their index at the end (for example AnimeWorld's) used to fail with "The video
  could not be loaded".

## 1.0.2

### Streaming sources are plugins
- The app no longer contains any streaming source. Sources are JavaScript plugins in Seanime's
  `onlinestream-provider` format, run in a sandboxed engine and installed from a repository
  link in **Settings → Plugins**, each with its own on/off switch.
- The official repository is `https://github.com/MaidLover69/Anilili_Linux_plugins`.
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
