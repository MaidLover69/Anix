## What and why

<!-- What does this change, and why? Link the issue: "Closes #123". -->

## How I tested it

<!-- Commands you ran, and what you checked in the app. Add screenshots for UI changes. -->

## Checklist

- [ ] `npx --no-install tsc --noEmit -p tsconfig.json` passes
- [ ] `npm run i18n:check` and `npm run i18n:test` pass; new text goes through `t()` with keys in `en.json`
- [ ] `cd src-tauri && cargo test --offline` passes (or the test files I touched, plus `cargo check`)
- [ ] New logic has a test
- [ ] UI changes checked in all 9 themes, at 960 px, with the keyboard, and right to left (Arabic)
- [ ] Database changes only add; nothing is dropped or renamed
- [ ] No streaming-site names, anti-bot bypasses, GPL options, secrets or personal paths in the diff
