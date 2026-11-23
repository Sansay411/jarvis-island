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
`/qa/notch-showcase.html` (every island state) and
`/qa/workspace-showcase.html?scene=workspace-active|workspace-empty|settings-voice|settings-behaviour`.

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

## Release

* `installer/` is a separate crate (the Windows setup window). Build it after
  `npm run tauri build` with `JARVIS_PAYLOAD=<path to *_x64-setup.exe>`; without
  it the window opens with nothing to install (handy for UI work).
* Update packages are signed: `npm run tauri build` needs
  `TAURI_SIGNING_PRIVATE_KEY` (the file `~/.tauri/jarvis-updater.key`) and
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (`~/.tauri/jarvis-updater.password`).
  CI has both as repository secrets. Losing the key means installed copies can
  no longer be updated.
* `git tag vX.Y.Z && git push --tags` runs `.github/workflows/release.yml`:
  Windows + both macOS arches, then `latest.json` and the release notes from
  `CHANGELOG.md`.
