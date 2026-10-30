# AGENTS.md

Guide for coding agents and contributors working on **Anix**. Read it before you change code.
If this file and the code disagree, the code wins. Fix this file in the same change.

## What Anix is

Anix is a Linux desktop app for anime. It has:

- a catalogue (search, discover, details)
- a tracker (watch history, watchlist, AniList/MyAnimeList/Comick sync)
- an airing schedule with release notifications and reminders
- a local library (scanned files matched to titles)
- downloads
- a player (embedded libmpv, or a web player)

Streaming sources are not built in. They come from **extensions**: JavaScript files the user adds
from extension repositories (see [Extensions](#extensions)). The extensions live in a separate
repository (`../plugins`, which has its own `AGENTS.md`).

- Version 1.3.1, identifier `io.anix.app`, licence MIT.
- Public site: https://anix-linux.vercel.app (download page, extension list, `version.json` for update checks).
- The app was first called **Anilili**. Old names still work and must keep working: `ANILILI_*`
  env vars, the `$anilili` plugin global, `anilili://` links, and old data folders (migrated by
  `src-tauri/src/migration.rs`).

## Architecture

```
React/TS UI (src/)  ──invoke()──▶  Tauri 2 commands (src-tauri/src/commands/)
     ▲  events (kebab-case)              │
     └───────────────────────────────────┤
                                         ▼
   catalogue/ (AniList first, then fallbacks)  providers/ + plugins/ (QuickJS extensions)
   accounts/ (OAuth, sync)   db.rs (SQLite)    stream_proxy.rs (loopback media proxy)
   downloads/   local_library.rs   embedded_mpv/ (libmpv, loaded at run time)
```

- **Frontend:** React 18, react-router 6, @tanstack/react-query 5, zustand 5, Tailwind 3, Vite 6, hls.js.
- **Backend:** Rust 2021 with Tauri 2, tokio, reqwest, sqlx (SQLite, WAL), rquickjs, oxc (TS → JS), tiny_http, zbus (MPRIS).
- **IPC:** the UI calls `#[tauri::command]`s through one hook per area in `src/hooks/useX.ts`, wrapped in react-query.
- **Network:** all HTTP goes through the shared clients in `src-tauri/src/clients/http.rs`, where DNS-over-HTTPS applies.
- **Window:** 1280×780 by default, minimum 960×600, with no system decorations (the app draws its own title bar).
- **CSP** (`tauri.conf.json`): `script-src 'self'`. `connect-src` is limited to `'self'`, `ipc:` and `http://127.0.0.1:*` (the stream proxy). `media-src` also allows `https:` and `blob:`. `frame-src` is only `https://www.youtube-nocookie.com`. Do not loosen it.

## Repo map

| Path | What |
|---|---|
| `src/main.tsx`, `App.tsx`, `router.tsx` | Entry (imports `theme/bootstrap.ts` first), app shell keyed on locale, routes + deep links + global hotkeys |
| `src/screens/` | Home, Discover, Detail, Watch, Library, Schedule, Downloads, Notifications, Settings |
| `src/components/` | Shared UI: `ui/`, `player/`, `library/`, `schedule/`, `settings/`, `setup/` (first-run dialog) |
| `src/hooks/` | One `useX.ts` per backend area (`invoke` + react-query) |
| `src/stores/` | zustand stores (`playerStore.ts`, `uiStore.ts`) |
| `src/lib/` | Pure helpers (`errorCodes.ts`, `languages.ts`, `streamProxy.ts`, `version.ts`, …) |
| `src/theme/` | `themes.ts`, `themes.css`, `accent.ts`, `bootstrap.ts`, `global.css` |
| `src/i18n/` | `core.ts` (pure rules), `index.ts` (`t`, `tn`, React side), `locales/*.json` |
| `src/types/` | Shared TS types (`AppSettings` in `index.ts`) |
| `src-tauri/src/lib.rs` | Module list, logging, Tauri builder, `generate_handler!` |
| `src-tauri/src/commands/` | Tauri commands, one file per area |
| `src-tauri/src/catalogue/` | Multi-source catalogue, fallbacks, circuit breaker, id mapping, schedule |
| `src-tauri/src/plugins/` | Extension runtime: `registry.rs`, `engine.rs`, `net.rs`, `prelude.js`, `provider.rs`, `health.rs` |
| `src-tauri/src/providers/` | `AnimeProvider` trait, racing/budgets/stream checks, title matching, sub/dub languages |
| `src-tauri/src/embedded_mpv/` | libmpv player (`ffi` dlopen, GTK GL layer, `policy`, `selftest`) |
| `src-tauri/src/stream_proxy.rs` | Loopback media proxy |
| `src-tauri/src/web_session.rs` | In-app webview sessions for the user's site verification |
| `src-tauri/src/db.rs` | Schema and migrations; app and config folders |
| `src-tauri/src/error.rs` | `AppError` and stable error codes (mirrored in `src/lib/errorCodes.ts`) |
| `src-tauri/src/i18n.rs` | Backend-shown text (notifications, tray, reminders) |
| `src-tauri/tests/` | Integration tests, `*_support/` helpers, `fixtures/` |
| `src-tauri/examples/plugin_run.rs` | CLI to run one extension in the real runtime |
| `src-tauri/resources/` | Anime4K shaders, hardware-decoding self-test clip |
| `scripts/` | `build-appimage.sh`, `build-libmpv-lgpl.sh`, `i18n-check.mjs`, `i18n-test.mjs`, `player-test.mjs` (embedded mpv stand-in) and `history-test.mjs` (when a visit counts as watch history), both run by `i18n:test` |
| `THIRD-PARTY-NOTICES.md` | Bundled components and licences |
| `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `SECURITY.md`, `.github/ISSUE_TEMPLATE/`, `.github/pull_request_template.md` | Contributor docs and forms. Keep them in step with the rules here |
| `docs/` | Gitignored and out of date (describes an older stack). Do not trust or copy it. |

## Setup, run, build, test

The checkout path may contain spaces. Quote it in every command.

Requirements: Node ≥ 22.6 (the i18n scripts use `--experimental-strip-types`), a current stable
Rust toolchain, and the Tauri 2 Linux system libraries (WebKitGTK 4.1, GTK 3). Optional:
system `libmpv.so.2` for the embedded player in debug builds, and `ffmpeg` for downloads.

| Purpose | Command (from the repo root) |
|---|---|
| Install | `npm ci` |
| Run the app (dev, hot reload) | `npx --no-install tauri dev` (starts Vite on port 1420) |
| Frontend only | `npm run dev` |
| Type-check | `npx --no-install tsc --noEmit -p tsconfig.json` |
| Frontend build | `npm run build` (`tsc && vite build`) |
| Debug build with the UI embedded | `npx --no-install tauri build --debug --no-bundle` → `src-tauri/target/debug/anix` |
| Rust check | `cd src-tauri && cargo check --offline` |
| Rust tests | `cd src-tauri && cargo test --offline` (one file: `--test <name>_tests`) |
| Live catalogue test (network) | `cd src-tauri && cargo test --test catalogue_tests -- --ignored --nocapture` |
| i18n checks | `npm run i18n:check` (keys, plurals, placeholders, raw UI text) |
| i18n / version tests | `npm run i18n:test` |

Notes:

- `plugin_host_reader_tests` reads `../plugins/src/<id>/provider.js`. It needs the extensions repo cloned next to this one, in a folder named exactly `plugins` (`git clone <extensions-repo-url> plugins`).
- `cargo build` / `cargo test` replace `target/debug/anix` with a dev-mode binary that loads `http://localhost:1420`. If you need the binary with the UI embedded, rebuild it with `tauri build --debug --no-bundle` just before you use it.
- Only one instance runs at a time (single-instance plugin). A second launch hands its arguments to the running window. Stop only the processes you started. Never kill every `anix` process.
- Debug-only switches: `ANIX_CATALOGUE_DOWN=anilist,myanimelist,…` simulates catalogue outages, and `ANIX_TEST_NOTIFICATION=1` shows a sample notification.
- `ANIX_PLAYER_ENGINE=web` or `--web-player` forces the web player. `--background` runs the release checks with no window.
- The only deep link is `anix://anime/<id>` (negative ids allowed), parsed in `src/router.tsx` `parseDeepLink`.

## Release build (overview)

1. `scripts/build-libmpv-lgpl.sh` builds an LGPL-only libmpv with pinned FFmpeg, mpv, libplacebo and libass. Run it once, or again after a version bump.
   Env: `ANIX_LIBMPV_DIR` (default `~/.cache/anix-build/libmpv`, no spaces) and `JOBS` (default `nproc`).
2. `scripts/build-appimage.sh` builds the AppImage. It copies the project to a folder without spaces, bundles the 64-bit GStreamer plugins and the LGPL libmpv, checks licences, and writes the result to `../release/`. It needs `rsync`, `python3` and `file`.

   | Env var | Meaning |
   |---|---|
   | `ANIX_SITE_URL` | Site used for update checks and the extension list. Baked in at compile time. Default: the public site. |
   | `ANIX_BUILD_DIR` | Build copy location (default `~/.cache/anix-build`, no spaces) |
   | `ANIX_LIBMPV_DIR` | Where step 1 put libmpv |
   | `ANIX_COMICK_CLIENT_ID` | Optional, compile time. Without it, the build has no Comick sign-in. Export it in your own shell. Never commit it. |

- Use the script for AppImages. A plain `tauri build` AppImage misses the bundled LGPL libmpv and may pick the wrong GStreamer plugins.
- Release AppImages are built by `.github/workflows/release.yml` in an Ubuntu 22.04 container: an AppImage only runs on systems whose glibc is at least as new as the build system's, and this base (glibc 2.35) covers Ubuntu 22.04, Mint 21, Debian 12 and everything newer. The workflow fails if the result needs a newer glibc. It runs when the version or the build scripts change, keeps the file as a 30-day artifact, and on a pushed `v*` tag also publishes it as a GitHub release with that version's CHANGELOG section as notes. The website copy (Drive, MEGA) is still uploaded by the owner. A local build on a new distribution only runs on systems as new as that one. The Comick client id comes from the repository secret `ANIX_COMICK_CLIENT_ID`.
- The script stops if the home path or user name appears anywhere in the package. Do not weaken that check. Only CI sets `ANIX_CHECK_USER=''`, because its account name (`root`) appears in unrelated binaries; the home path is still checked there.
- RPM and DEB builds bundle no libmpv and default to the web player.

## Hard rules

### Security and privacy
- **No secrets or personal data** in code, tests, fixtures, logs or docs. That means no tokens, passwords, client secrets, emails, user names or home paths. The OAuth client ids already in `commands/auth.rs` / `commands/mal.rs` are public PKCE ids. Do not copy them anywhere else. Build-time ids come from env vars.
- Account tokens live in `settings.json` (mode 0600). `get_settings` / `save_settings` (`commands/settings.rs`) call `redact_secrets` / `keep_stored_account_keys` in `accounts/store.rs`, so tokens are redacted on read and the stored ones are kept on save. Keep both paths intact when you add a secret key.
- Diagnostics reports (`diagnostics.rs`) must stay free of private data.
- **Never write code that bypasses anti-bot checks.** No captcha solving, no solver services, no faked browser fingerprints, no hidden challenge execution. When a site shows a check, the request falls back to the in-app browser session. If that fails, the user passes the check themselves in a real webview through the **Verify** flow (`web_session.rs`, `plugins/net.rs`).

### Stream proxy (`stream_proxy.rs`)
- Binds `127.0.0.1` on a random port. Every path starts with a 128-bit random token, and a wrong token gets 403. It is never an open proxy.
- Only `GET`/`HEAD`/`OPTIONS`. Remote targets must be absolute `http(s)` URLs with a host, on a public address (`clients/net_guard.rs`, see Extensions › Network). Tests with loopback upstreams use `start_allowing_private_upstream()`.
- Local files are served **only** by a registered 32-hex key (`local_media::playable_path`), never by path.
- The embedded player loads only stream-proxy URLs. Keep these limits when you add endpoints, and add a test in `stream_proxy_tests.rs`.

### Embedded mpv and licensing
- libmpv is loaded at run time with `dlopen` (`embedded_mpv/ffi.rs`). It stays dynamically linked and replaceable.
- Ship **only LGPL builds** of libmpv and FFmpeg. Never add `--enable-gpl`, `--enable-nonfree` or mpv `-Dgpl=true`, and never bundle a distro's GPL mpv. Release builds refuse a GPL/nonfree libavcodec (`is_gpl_license`). Debug builds only warn.
- The page controls playback only through the fixed `Action` enum, never with raw mpv commands. Any start-up failure falls back to the web player.
- When you change a bundled component version, update `THIRD-PARTY-NOTICES.md` in the same change.

### Catalogue and ids
- **AniList ids are the internal key** for titles everywhere (DB, routes, deep links, extensions).
- When AniList is unreachable, the catalogue falls back in this order: MyAnimeList → Kitsu → Shikimori → Comick → Jikan, then IMDb suggestions for search. Each source gets 7 s, and `breaker.rs` skips failing sources. If none answers, the app uses the saved answers in `src-tauri/src/offline.rs`.
- A title with no known AniList id gets a **negative stand-in id**: `-(site × 100_000_000 + native id)`, with 1 = MAL, 2 = Kitsu, 3 = IMDb (`catalogue/ids.rs`). Use `synthetic_id` / `decode`. Never invent another scheme, and never assume an id is positive.

### Database
- **Migrations only add.** Add tables, columns and indexes (`CREATE … IF NOT EXISTS`, `ALTER TABLE … ADD COLUMN`). Never drop or rename user data. All migrations run in one transaction in `db::run_migrations`. When a layout must change, copy the rows into the new layout and keep anything you cannot understand under another name (see `migrate_watch_history`).
- Settings files are written atomically (tmp + rename). A file that cannot be read is moved aside as `*.corrupt`, never overwritten.

### UI: themes, motion, access, layout
- **All 9 themes** must look right: `dark`, `oled`, `light`, `mocha`, `hacker`, `dracula`, `tokyo-night`, `manga`, `comic`. Use the colour tokens (Tailwind `bg-*`, `border-*`, `accent-*` → CSS variables in `themes.css`). Never hard-code colours. Check contrast in `light` and `manga` as well as the dark themes.
- **Reduced motion:** `global.css` turns animation off under `prefers-reduced-motion: reduce`. JS-driven motion (`scrollTo({behavior})`, timers, carousels) must check the media query itself.
- **Keyboard and screen readers:** every control must be reachable and usable with the keyboard and show a visible focus ring (`:focus-visible` styles exist). Use real `<button>`s and links, `aria-label` on icon-only buttons, and dialog/listbox/menu roles with focus handling like `components/ui/Modal.tsx`.
- **Layouts:** the UI must work at the 960 px minimum window width and at wide sizes (content is capped at `max-w-[1600px]` in `router.tsx`). The UI must also work in RTL (`ar`).

### i18n
- **Every user-visible string goes through `t('key', vars)`** (or `tn` for text that has elements inside) from `src/i18n`. That includes `aria-label`, `title` and `placeholder`. Add the key to `locales/en.json`, which is the source and fallback. Other catalogs may lag behind.
- Plurals are objects keyed by `Intl` plural category. Use `{name}` placeholders. Never build sentences out of pieces.
- Never call `t()` at module load. Changing the language re-mounts the app.
- Backend-shown text (notifications, tray, reminders) goes in `src-tauri/src/i18n.rs` for all 12 languages. Log lines stay in English.
- Run `npm run i18n:check` and `npm run i18n:test` before you finish.

## Extensions

- Format: the `onlinestream-provider` extension format (`type: "onlinestream-provider"`). The flow is `search` → `findEpisodes` → `findEpisodeServer`, plus `getSettings()`.
- Manifest fields: `id` (`[A-Za-z0-9_-]`, ≤ 64), `name`, `version`, `author`, `description`, `type`, `language` (`javascript` or `typescript`, transpiled with oxc), `lang`, optional `languages {sub?, dub?}` (ISO 639-1), `icon` (`https://` or `data:image/`), `payload` or `payloadURI`, `userConfig`, `changelog`.
- Repositories: an https JSON list of manifests, `{name, plugins}`, or one manifest. Limits: 2 MB each for index, manifest and payload, and at most 200 plugins. `file://` repos are allowed only with `ANIX_DEV_PLUGIN_REPOS=1`.
- **Sandbox** (`plugins/engine.rs`): one QuickJS runtime per plugin on its own thread, with memory/stack limits and an interrupt. No module loader, file system or process access. Load ≤ 10 s, timers ≤ 60 s, `$sleep` ≤ 2 s.
- **Network** (`plugins/net.rs`): through the shared clients (DoH). Timeout 15 s by default, 30 s max. Responses ≤ 16 MB, requests ≤ 4 MB. **Private and loopback targets are denied** (`clients/net_guard.rs`): by the URL's host, by a resolver that drops private DNS answers before connecting, and on every redirect hop. The same guard covers the stream and subtitle links extensions return (stream checks in `providers/manager.rs`, the stream proxy's upstream, subtitles fetched for external players). `plugins/provider.rs` also drops stream and subtitle links whose host is written as a private address or `localhost`, so they never reach downloads or external players. Do not add a way around that, except `plugin_run --allow-local` and the test allowances (`allow_private_network`, `AnimeProvider::allows_private_network`).
  - Known limits: requests made through the in-app browser session (`self.session.fetch` in `net.rs`) are checked only by the URL's host before sending; WebKit resolves names and follows redirects itself. ffmpeg, mpv and VLC resolve stream host names themselves, so a host name that resolves to a private address is not caught for downloads and external players. A proxy the user set in `HTTP(S)_PROXY`/`ALL_PROXY` is resolved without the filter (`net_guard::proxy_hosts`), so a local proxy given by name keeps working.
- Globals live in `plugins/prelude.js` (`fetch`, `LoadDoc`, `CryptoJS`, `$store`, `$anix`, …). A new global needs a test in `plugin_engine_tests.rs`.
- Try an extension in the real runtime:
  ```sh
  cd src-tauri && cargo build --offline --example plugin_run
  target/debug/examples/plugin_run ../../plugins/src/<id>/manifest.json play "<title>" 1 [--dub]
  ```
  Commands: `settings`, `search`, `episodes`, `server`, `list`, `play`, `repo <link>`. JSON goes to stdout and plugin logs to stderr. Exit code 1 means the call failed; 2 means a usage error.

## Coding conventions

- **Comments:** Rust files open with a `//!` module doc in plain English saying what the module does and why. UI paths are written like "Settings › System". Public items get `///` docs, and TS files a `/** */` header. Comment the *why*. Put units in constant names (`_S`, `_BYTES`) or use `Duration`. Do not leave `TODO`/`FIXME` comments.
- **Errors:** core code uses `AppError` (thiserror). Tauri commands return `Result<T, String>` with a user-facing sentence. Stable error codes come from `error.rs::code_for_message`, mirrored in `src/lib/errorCodes.ts`, so change both together. Non-fatal problems go to `tracing::warn!`.
- **Naming:** Rust snake_case / PascalCase; commands are snake_case. React components are PascalCase `.tsx`, hooks `useX.ts`, events kebab-case (`plugins-changed`).
- **Frontend data:** use the area's hook in `src/hooks/` (react-query) for backend data, zustand only for UI/player state, and pure helpers in `src/lib/`. Settings defaults are in `useSettings.ts` `DEFAULT_SETTINGS`, and the type is `AppSettings`.
- Keep changes small and in the existing structure. Do not add a dependency for a few lines.

## Tests

New logic needs a test in the same change.

- **Rust:** integration tests in `src-tauri/tests/<area>_tests.rs` against `anix_lib`'s public API. Shared helpers go in `*_support/mod.rs`, recorded data in `fixtures/`. Use `#[tokio::test]` for async code.
  - No live network. Use loopback servers on `127.0.0.1` and in-process fakes. A test that needs the network must be `#[ignore]`.
  - Name tests as sentences describing the behaviour (`loads_only_from_the_stream_proxy`).
  - Cover the refusal paths for anything at a trust boundary (proxy, plugin network, file paths, deep links, external players).
- **TypeScript:** there is no test runner. Keep logic in pure functions in `src/lib/` or `src/i18n/core.ts`, and add `node:assert/strict` cases to `scripts/i18n-test.mjs` (or a similar node script).
- **DB:** schema changes need a case in `db_tests.rs` that opens an older layout and checks that no rows are lost.

## Commits

Conventional Commits, lowercase type, imperative mood, for example:

```
feat(player): remember subtitle delay per title
fix(ci): install the WebKitGTK headers
docs: explain the stand-in id scheme
```

One logical change per commit. Never commit secrets, build output (`dist/`, `target/`, `release/`) or personal paths.

## Before you finish

- [ ] `npx --no-install tsc --noEmit -p tsconfig.json` passes
- [ ] `cd src-tauri && cargo test --offline` passes (or the touched `--test` files, plus `cargo check`)
- [ ] `npm run i18n:check` and `npm run i18n:test` pass; no raw user-visible strings
- [ ] UI changes checked in all 9 themes, with reduced motion, with the keyboard, at 960 px and wide
- [ ] New logic has tests, including refusal paths at trust boundaries
- [ ] No GPL/nonfree options; `THIRD-PARTY-NOTICES.md` matches bundled versions
- [ ] Migrations only add; old names (`anilili`) still work
- [ ] No secrets, tokens, personal data or machine paths in the diff
- [ ] This file still matches the code
