# Notes for coding agents

## Verify a change

```bash
cd src-tauri && cargo test --lib      # Rust unit tests (voice gate, media grammar, agent, store…)
cd src-tauri && cargo check           # the app itself, no warnings expected
npx tsc --noEmit                      # TypeScript
npx vitest run                        # frontend unit tests
npx vite build                        # production frontend bundle (dist/ is git-ignored)
```

Visual QA without the native app: `npx vite --port 1420`, then open
`/qa/notch-showcase.html` (every island state; `?preset=growth`,
`growth-card`, `nook-events`, `event-soon`, `event-now`, `compact-soon`,
`gain`, `moment-day`, `moment-level`, `focus-cal` for the growth and
calendar states) and
`/qa/workspace-showcase.html?scene=workspace-active|workspace-empty|settings-voice|settings-behaviour`,
`/qa/planner.html?view=day|4day|week|month&plan=1` (the Google-style planner
with a sample week) and `/qa/reset.html?reason=budget|strict|voluntary&screen=feed|games|workout|breath|step|done|started&plan=1`
(«Перезагрузка»; `&reward=level|record` for the other «Готово» screens).

## Island growth and calendar

* `store/agenda.ts` mirrors the planner's events for the island (a week back
  to four weeks ahead, reloaded on `planner:changed`) and decides the
  reminder card and the compact count-down (`planner.islandRemind` minutes,
  shown once per block and phase, remembered in `localStorage`). Its pure
  helpers (`dayRows`, `upcoming`, `focusBlock`) have tests.
* Growth moments come from comparing two `mentor:state` payloads
  (`momentOf`); the backend re-emits the state after card answers, finished
  focus sessions and done tasks (`hub.rs` → `growth_changed`).
* The 3D icons in `src/mentor/art/` are 256 px WebP cut from Codex renders
  (512 px masters, same clay style as `installer/ui/hero.png`); keep new ones
  in that style and size.

## Gotchas

* Line endings are mixed in the repo (`git ls-files --eol`): some files are CRLF
  in the index (e.g. `platform/media.rs`, `SettingsApp.tsx`), most are LF. Keep
  each file's own style — a Python text-mode write on Windows turns LF into CRLF.
* On 2026-11-21 files had gone missing inside `node_modules` and in
  `src-tauri/target/debug/build/*/out`. Symptoms: Vite «Cannot find module
  './lib/source-map-consumer'», `tsc` errors about `MotionStyle`, cargo
  «couldn't read …/out/bindgen.rs». Fix the cache, not the code:
  `cargo clean -p <crate>`; for npm, install the lockfile into a temp folder
  and copy over only the missing files.
* Voice behaviour is decided in `src-tauri/src/voice/intent.rs` (whose words
  are these) and `agent/fast.rs` (exact local commands). Their tests use real
  phrases from the logs; add new live misfires there first.
* The repo moved from `C:\Users\-pin\Documents\Jarvis` to `E:\Jarvis` on
  2026-11-22. Build-script outputs in `src-tauri/target/*/build/*/output`
  still pointed at the old path, so a crate that had to rebuild (after a
  `windows` feature change) failed with «could not find native static library
  `WebView2LoaderStatic`». Fix: `cargo clean -p <crate>` (and `--release`).
* Files written by the editor tool come out CRLF; most of the repo is LF
  (`sed -i 's/\r$//' file` before committing a new file).

## Planner, mentor, Telegram

* `planner/` keeps events as local wall-time strings (`YYYY-MM-DDTHH:MM`,
  all-day `YYYY-MM-DD`, ends exclusive) so range queries are string compares;
  `planner/google.rs` mirrors the primary calendar (push first, then a
  windowed pull: 35 days back, 120 ahead). Unit tests cover the mapping and
  the reconcile rules without network.
* `mentor/guard.rs` polls the foreground window every 2 s; the address bar is
  read over UIA only when the title changes on YouTube/Instagram/VK. Closing
  uses the selected tab via UIA, Ctrl+W only if the same page is still in
  front. `mentor/seed.rs` holds the offline cards (checked by a test).
* `telegram/` long-polls inside the app; one bound chat (`/start CODE` from
  Settings). Never log Bot API URLs: the token is in them.

## Release

* `installer/` is a separate crate with two windows sharing `src/lib.rs`:
  `jarvis-setup` (embeds the NSIS package) and `jarvis-uninstall`. Order:
  1. `cargo build --release --manifest-path installer/Cargo.toml --bin jarvis-uninstall`
  2. `npm run tauri build` — `src-tauri/windows/hooks.nsh` packs that exe and
     makes it `UninstallString` (skipped with no error if it isn't built);
  3. `cargo build --release --bin jarvis-setup` in `installer/` with
     `JARVIS_PAYLOAD=<path to *_x64-setup.exe>`; without it the window opens
     with nothing to install (handy for UI work).
* The uninstall window forwards any `/S`, `/P`, `/UPDATE` or `_?=` call to the
  NSIS `uninstall.exe` unchanged (updates call `UninstallString` that way).
  UI work: `target/debug/jarvis-uninstall.exe --demo` (debug builds only,
  touches nothing), or serve `installer/ui/` and open `uninstall.html`. Never
  run it without `--demo` against the real install: it finds Jarvis through
  the registry and removes it. To test the real flow, make a folder with an
  empty `jarvis.exe`, a stand-in `uninstall.exe` and the uninstaller, and
  untick both boxes (the data step uses the real `%LOCALAPPDATA%`).
* Windows signing (SmartScreen/antivirus): planned through SignPath Foundation
  (free for OSS, signs only GitHub Actions builds, each release approved by
  hand). Everything that runs needs a signature: `jarvis.exe`,
  `jarvis-uninstall.exe`, the NSIS package and its `uninstall.exe`, and
  `jarvis-setup.exe`. Don't add packers (UPX) or self-deleting scripts.
* Update packages are signed: `npm run tauri build` needs
  `TAURI_SIGNING_PRIVATE_KEY` (the file `~/.tauri/jarvis-updater.key`) and
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (`~/.tauri/jarvis-updater.password`).
  CI has both as repository secrets. Losing the key means installed copies can
  no longer be updated.
* `git tag vX.Y.Z && git push --tags` runs `.github/workflows/release.yml`:
  Windows + both macOS arches, then `latest.json` and the release notes from
  `CHANGELOG.md`.
