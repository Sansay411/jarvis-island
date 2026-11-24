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

## Jarvis Cloud (`cloud/`)

* A Cloudflare Worker (`jarvis-cloud`, https://jarvis-cloud.credo-x5.workers.dev)
  with a D1 database of daily counters. It holds `DEEPSEEK_KEY`, `YANDEX_KEY`
  (SpeechKit, Kazakhstan installation) and `TOKEN_SECRET` as Worker secrets:
  never put them in the repo, the app or logs. Change one with
  `cd cloud && npx wrangler secret put NAME`; limits and prices are `vars` in
  `cloud/wrangler.jsonc` (deploy: `npx wrangler deploy`, migrations:
  `npx wrangler d1 migrations apply jarvis-cloud --remote`).
* Routes: `POST /v1/register` → device token `j1.<uuid>.<hmac>`;
  `/v1/chat/completions` and `/v1/tools/{search,fetch}` (DeepSeek, model forced
  to `deepseek-flash`, output capped), `/v1/tts` (SpeechKit v3 body, streamed),
  `/v1/stt` (raw 16 kHz LINEAR16 ≤ 30 s → `{text}` through the async v3 API:
  Kazakhstan has no sync REST recognition). Caps answer 429 with
  `error.code = "jarvis_limit"`; the app shows that message instead of retrying.
* App side: `src-tauri/src/cloud.rs` (token in the credential store as
  `JARVIS/cloud-token`, STT race with GigaAM), preset `jarvis-cloud` (the
  default), DeepSeek without an own key falls back to it, Yandex voices without
  a key go through it. Pure Worker helpers are tested by the root `npx vitest run`
  (`cloud/test`); `cd cloud && npm ci && npm run typecheck` checks the Worker.
* SpeechKit recognition needs the role `ai.speechkit-stt.user` on the service
  account (synthesis: `ai.speechkit-tts.user`); without it `/v1/stt` answers 503
  and the app stays on GigaAM for 10 minutes at a time.

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

## Voice: instant commands

* `voice/vad.rs` runs a second Silero detector with a 0.28 s end pause
  (`QUICK_SILENCE_S`). For phrases up to 2 s it sends `VadEvent::Quick`;
  `local.rs` decodes it and, if `intent::quick_final` says the words are an
  exact player/device command or stop words, answers at once and drops the
  rest of the utterance (`consumed_seq`). Everything else still waits for the
  0.75 s end.
* `fast::bare` collapses one word said several times («дальше дальше»).
  Player words that need an open player are in `NEEDS_PLAYER` (`старт`,
  `вперёд`, `другую`). Without the name, dormant, exact player words and
  short sound commands (`intent::device_word`: «громче», «тише»,
  «громкость 30») act silently (`quiet_next`).

## Planner and the agent

* `planner/suggest.rs`: the model plans from the user's `wishes` (IPC
  `planner_suggest { date, wishes }`, the «Что важно в этот день?» box,
  Telegram «план дня: …»); the fixed plan is only the no-model fallback.
* `calendar_manage` has `free` (free windows for up to 7 days) and `plan`
  (several blocks at once, `check_blocks` skips clashes with a reason).

## MCP catalog

* `src/settings/mcpCatalog.ts`: the one-click catalog in Settings →
  «Подключения». Every package is pinned to a release at least a week old
  (check `npm view <pkg> time` / PyPI before bumping); `{field}` placeholders
  are filled from the card's inputs; env/header values go to Credential
  Manager through `mcp_save`. `mcpCatalog.test.ts` checks ids, placeholders
  and pins.

## Landing (`site/`)

* Static pages in `site/public` plus `site/src/worker.ts` (Cloudflare Worker
  `jarvis-site`): `/download/{windows,mac-arm,mac-intel,portable}` redirect to
  the newest GitHub release asset and count downloads per day in D1;
  `/api/waitlist` stores Pro e-mails. `cd site && npm ci && npx wrangler dev`
  (local D1: `npx wrangler d1 migrations apply jarvis-site --local`).
  Before the first deploy: `npx wrangler d1 create jarvis-site`, put the id
  into `site/wrangler.jsonc`, apply migrations with `--remote`.
* Prices and the Paddle client token / price ids live in
  `site/public/config.js`; without them «Получить Pro» opens the waitlist.
* Site texts never use long dashes (—, –) or «--»: the owner's rule.
* Product shots in `site/public/img/ui` are real app screens from the QA
  pages (`/qa/notch-showcase.html?preset=…`, workspace, planner) captured at
  3x with a transparent background; 3D models are Codex renders exported at
  640 px (1100 to 1400 px for the hero, app icon and factory).

## Mentor cards and games

* `mentor/gen.rs` makes 500+ cards by rule at every start (stable `gen-*`
  ids, `INSERT OR IGNORE`): code output puzzles with computed answers, the
  `mind` track (logic, sequences, mental math, clocks, roman numerals) and
  irregular verbs. Add a template, not hand-written answers; the test checks
  every card with `valid_card` and that options are distinct.
* The model writes 12 cards per batch around the last mistakes
  (`generate_for`): in the background every 3 h per track and when the feed
  opens (`mentor_generate`, once in 10 min per track).
* `src/mentor/Puzzles.tsx`: sudoku (4×4/6×6 with the clay icons, 9×9 in
  three levels, unique solution, notes, hints), Schulte table, memory pairs.
  QA: `/qa/reset.html?screen=game&game=sudoku|schulte|memory`.

## Microphone stage (quiet and noisy mics)

* `voice/conditioner.rs`: high-pass 90 Hz → speech-gated AGC (target
  -20 dBFS, -6…+30 dB, soft limiter) → RNNoise (`nnnoiseless`, 48 kHz,
  480-sample frames) → 16 kHz. Each `MicChunk` carries `vad` (denoised) and
  `stt` (0.65 denoised mix in «Обычное», all denoised in «Сильное»); use
  `Vad::push_pair`. Utterances are peak-normalised (-3 dBFS, ≤ +24 dB) before
  recognition. Learned gain per device: store key `voice.mic_gains`.
* `vad::Floor` echo guard is relative (floor + 12 dB and the user's speech
  level - 25 dB); never add absolute dB thresholds back.
* `voice/miclab.rs`: `voice_mic_monitor` (keepalive every 2 s) and
  `voice_mic_test`; while a test runs the session ignores audio.
  Settings → Голос → «Микрофон» (`MicCheck.tsx`), QA
  `/qa/workspace-showcase.html?scene=settings-voice&mic=result`.
* Measuring: `cargo test --lib live_quiet_noisy_speech -- --ignored --nocapture`
  (real models); `JARVIS_VOICE_INPUT_ATTEN_DB=25` makes the debug WAV feeder
  quieter.

## Overlay, teach mode, cards, area

* `overlay.rs`: one click-through `overlay` window, created on demand,
  excluded from capture. Items (`ring`, `arrow`, `caption`, `card`) use
  physical screen pixels; API `overlay::draw(app, items, replace)` /
  `overlay::clear(app)` (never from a sync command on the main thread). On
  Windows don't call `show()/hide()/set_position()` on it: use the main-thread
  helpers. DOM nodes with `data-overlay-interactive` get the mouse (rects go to
  Rust, a ~30 Hz tracker toggles click-through).
* Tools `screen_point`, `screen_clear`, `guide_step` (teach mode: blocks
  ≤ 150 s for the user's click, tool timeout 170 s, never clicks itself).
  Targets: UIA element or coords of the last screenshot
  (`screen::to_screen_rect`). QA `/qa/overlay.html?scene=ring|guide|arrow|card|done`.
* Cards: `src/overlay/cards/*` (`normalize.ts`, `flowLayout.ts`, `math.ts`,
  `CardView`) and `src-tauri/src/cards.rs` must stay in step (both normalise
  idempotently); tools `explain_card`/`explain_clear` in `tools/explain.rs`.
  The tool result omits the card; chat/history rebuild it from the stored
  arguments (`cards::card_json`). QA `/qa/cards.html?bg=dark|light&mode=compact`.
* Area: `area.rs` + `areapick` window (shortcut `Ctrl+Alt+A`), region capture
  in `platform/capture.rs`; the pending area is attached as an image part to
  the next chat or voice request. Circling with the cursor while listening
  (`detect_circle`, setting `voice.circleToAsk`) attaches the bbox and rings
  it on the overlay. QA `/qa/areapick.html`.

## Skills, connections, repeats, models, welcome, looks

* Skills: `skills/` (`SKILL.md` with front matter in the app data dir), tools
  `skill_save|find|read|delete|install` (install is always confirmed and
  grants nothing), index injected into the prompt, Settings → «Навыки».
* MCP catalog data lives in `src/settings/mcpCatalog.json` (read by Rust via
  `include_str!` in `mcp/catalog.rs`, with RU/EN `aliases`). Chat connects via
  `tools/connect.rs` (`connect_find`, `connect_service`, …); secrets go only
  through the island key prompt (`mcp:secret-prompt` / `mcp_secret_submit`),
  never through the model. QA `/qa/notch-showcase.html?preset=secret`.
* Repeats: `repeat.rs` (`next_after`, `describe_rule`, tested); a repeating
  item moves to its next time as it fires (an unacknowledged alert never stalls
  the series); missed by > 10 min → skipped silently. `reminder_skip` skips one.
* Local models: `ai/probe.rs` (`model_probe`, Ollama `/api/show`+`/api/ps`,
  LM Studio `/api/v0/models`, optional active test). Turn start probes
  local/custom presets with a 3 s cap and caches per session.
* Welcome: `welcome` window, `src/welcome/*`; shown on first start only when
  no persona name, no memories, not done and < 3 dismissals. QA `/qa/welcome.html?step=0..5`.
* Looks: `src/island/skins/*`, setting `appearance.skin`
  (`amber|orb|ice|pixel|minimal`). A new built-in tool must be added to
  `src/store/toolNames.json` and `TOOL_ACTIVITY` in `src/store/emote.ts`
  (`tools::tests` and `emote.test.ts` fail otherwise). QA
  `/qa/notch-showcase.html?preset=skin-<id>` or `?skin=<id>&activity=<kind>`.
* New windows (`areapick`, `overlay`, `welcome`) must be listed in
  `src-tauri/capabilities/default.json`.
