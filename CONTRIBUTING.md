# Contributing to Anix

Thanks for wanting to help! Bug reports, translation fixes and small, focused pull requests are all welcome.

By taking part you agree to the [Code of Conduct](CODE_OF_CONDUCT.md). Security problems go through
the private process in [SECURITY.md](SECURITY.md), not public issues.

## Ways to help

- **Report a bug:** open an issue with the **Bug report** form. Attach the diagnostics report from
  **Settings › System › Diagnostics report**. It removes logins, tokens, email addresses and your home folder.
- **Fix a translation:** the interface was translated by AI, so corrections from native speakers
  are very welcome. Use the **Translation fix** form, or edit `src/i18n/locales/<language>.json`
  directly in a pull request.
- **Suggest a feature:** open a **Feature request** first and wait for a reply before you write
  code. Anix prefers improving what exists over adding new features, so not every idea gets in.
- **Source requests and extensions:** this repository is for the app only. Requests for new
  provider extensions or metadata sources, and problems with the extensions maintained for Anix, go to the
  [extensions repository](https://github.com/MaidLover69/Anix_plugins/issues). Problems with a
  third-party extension go to its author.

## Setting up

You need Node 22.6 or newer, a current stable Rust toolchain, and the Tauri 2 Linux libraries
(WebKitGTK 4.1, GTK 3). Optional: a system `libmpv.so.2` for the embedded player, and `ffmpeg` for downloads.

```sh
npm ci
npx --no-install tauri dev          # the app with hot reload
```

The full guide, with the repository map, architecture and every rule, is [AGENTS.md](AGENTS.md).
Read it before larger changes.

## Before you open a pull request

Run the checks that cover what you changed:

```sh
npx --no-install tsc --noEmit -p tsconfig.json
npm run i18n:check && npm run i18n:test
cd src-tauri && cargo test --offline
```

And make sure that:

- every user-visible text goes through `t('key')`, with the key added to `src/i18n/locales/en.json`;
- new logic has a test (`src-tauri/tests/<area>_tests.rs`, or `scripts/i18n-test.mjs` for TypeScript helpers);
- UI changes work in all 9 themes, at the 960 px minimum width, with the keyboard, and right to left (Arabic);
- database changes only add (new tables or columns), never drop or rename user data.

## Rules that are not up for discussion

- **No streaming sources or scrapers in this repository.** Sources come from extensions only. Do not add
  site integrations, streaming links or scrapers to the core app.
- **No anti-bot bypasses:** no captcha solvers, faked browser fingerprints, or hidden challenge
  solving. Users pass checks themselves through the in-app **Verify** window.
- **LGPL only** for the bundled libmpv and FFmpeg. Never enable GPL or nonfree options.
- **No secrets or personal data:** no tokens, client secrets, emails, user names or home paths in
  code, tests, logs or docs.

## Commits and pull requests

- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org):
  `fix(player): keep the subtitle delay after a seek`.
- One logical change per pull request. Explain what changed and why, and how you tested it.
  Screenshots help for UI changes.
- Never commit build output (`dist/`, `target/`, `release/`).

## Licence

Anix is MIT-licensed. By contributing, you agree that your contribution is released under the
same licence.
