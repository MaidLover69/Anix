# CONSTRAINTS.md

The quality bar for Anix. Read it before you write code. A change that cannot meet it is fixed, not let through
by editing this file. `npm run floor` (`scripts/floor-guard.mjs`) checks the diff against it and runs in CI.

## Floor (blocking)

A change may not add any of these. The floor guard flags them in added lines only, so old code is not blocked.

- Silenced checkers: no new `@ts-ignore`, `@ts-nocheck`, `@ts-expect-error`, `eslint-disable` or similar.
- Rust `#[allow(...)]`: only with a reason on the same line, `#[allow(x)] // why` or `#[allow(x, reason = "why")]`;
  the same for `#[expect(...)]`. A lint set to `"allow"` in `Cargo.toml` needs a `#` reason on its line or the one above.
- Unfinished work: no `todo!()`, `unimplemented!()`, "not implemented" throws, empty `catch {}`, or `TODO` comments.
- Tests made easier: no new `#[ignore]` (except tests that need the network, marked `#[ignore = "needs network"]`,
  see AGENTS.md › Tests), `.skip(`, `.only(`.
- Tests removed: no deleted test file, and no net loss of assertions in a test file.
- Secrets and personal data: none, anywhere (AGENTS.md › Security and privacy).
- This file: no rule removed and no number loosened in the same change as the code that needed it.

An exception needs a dated row below with a reason and an owner; its Rule cell names the guard rule it lets through
(the word in brackets in the guard's output, such as `threshold-loosened`). The guard prints new rows so a reviewer
sees them, and lets through only that rule, only in the change that adds the row.

## Measured numbers (ratchets)

Measured on 2026-10-31. A limit may only move in its direction. When a change makes it better, write the new
number here in the same change. The guard reads the limit, not the `(measured …)` note, so the note can be updated freely.

| Metric | Today | Direction |
|---|---|---|
| Main JS chunk (`dist/assets/index-*.js`) | 570043 bytes, must not grow (screens other than Home load on first visit, 2026-11-01) | down |
| Shell ready after launch, web player | at most 3.0 s (measured 2.4–2.6 s) | down |
| Idle CPU on Home, web player | at most 1 % (measured 5.3–6.4 % in front while the banner turns, was 9–12 %; 0 % in the background or with Fewer effects), must fall | down |
| Idle CPU on Home, embedded mpv | at most 6 % (measured 4.1 % in front, was 7–15 %; 0 % in the background or with Fewer effects), must fall | down |
| Memory after a 2-minute browse, web player | at most 560 MB (measured ≈536 MB) | down |
| Memory while playing, web player | at most 950 MB (measured 730–900 MB) | down |
| First frame of a stream, web player | at most 12 s (measured 8–11 s) | down |
| Processes or mpv sockets left after closing | at most 0 | down |

Numbers come from a debug build driven through WebKitWebDriver with a fresh profile, on the
reference machine (4 cores, Intel UHD, Fedora, niri): one run of the measuring script plays an
episode through the extensions for 60 s. Network-bound numbers (first frame) vary with the sites;
compare medians of three runs.

## External checks

- Dependencies: `cargo audit` on `src-tauri/Cargo.lock` and `npm audit --omit=dev`, weekly in CI.
- Licences: LGPL-only libmpv and FFmpeg (AGENTS.md › Embedded mpv and licensing).

## Exceptions

| Id | Date | Rule | Reason | Owner |
|---|---|---|---|---|
| E1 | 2026-11-01 | assertion-removed | Dead code removed in 1.4 took one assert each with it: `clear_match` (no caller) in local_library_tests, the `SourcesUnavailable` error (now the plain APP-ERR case, still asserted in the table) in unit_tests, and the test-only `decode_text` wrapper in web_session_tests (charset lookup still asserted; decoding checked end to end in plugin_verification_tests `/guarded/latin`). | maintainer |
| E2 | 2026-11-01 | dependency-audit | `cargo audit` ignores RUSTSEC-2023-0071 (rsa, Marvin timing attack, no fixed release). rsa is in Cargo.lock only through sqlx's MySQL driver, which Anix never compiles (`cargo tree -i rsa --target all` is empty), so no shipped code contains it. Drop the ignore when sqlx no longer locks rsa or a fixed rsa exists. | maintainer |
