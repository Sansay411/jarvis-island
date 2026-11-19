# Оживить разделы острова — план реализации

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Переходы с направлением между разделами острова, Полка плитками с превью/drag-out/«В Telegram», Стрик с волной и count-up, Фокус с крупным кольцом, пресетами и flip-цифрами.

**Architecture:** Чистые функции анимаций и форматирования — в `src/island/motion.ts` (покрыты vitest). Компоненты разделов (`PeekView`, `TrayDeck`, `ActivityDeck`, `FocusDeck`) используют их через `motion/react`. В Rust две новые команды: `shelf_thumbnail` (переиспользует `filekit::info_of`) и `shelf_drag_out` (нативный OLE drag через `SHDoDragDrop`). Отправка в Telegram — уже существующая `files_action` с action `"telegram"`.

**Tech Stack:** React 19 + TS, `motion/react` 14, Rust + Tauri 2.12, windows-rs 0.62 (`Win32_UI_Shell`, `Win32_System_Ole`, `Win32_System_Com` уже включены), vitest (новая dev-зависимость).

**Spec:** `docs/superpowers/specs/2026-11-19-island-decks-alive-design.md`

## Global Constraints

- Анимации только разовые, по событию; через 1 с после открытия раздела активных анимаций нет.
- `prefers-reduced-motion` (`useReducedMotion()`): волна, flip и blur → простое появление opacity.
- Янтарь только: сегодняшняя клетка, кольцо фокуса, цель «В Telegram» при наведении с файлом.
- Bone (`--bone`) для выбранного пресета и основных кнопок; шрифт Onest; числа `font-variant-numeric: tabular-nums`.
- Никаких фейковых данных в `src/island/**`; демо-данные только в `src/qa-notch.tsx`.
- Остров не забирает фокус у окна пользователя.
- UI-тексты по-русски.
- Ветка содержит чужие незакоммиченные изменения: в каждом коммите `git add` только файлы задачи; коммитить только с разрешения пользователя (иначе шаг «Commit» пропускается и отмечается).

## Review Focus

1. Файл на полке удалён/перемещён (`exists: false`) — плитка показывает иконку типа и приглушена, drag-out и «В Telegram» для неё отключены, без ошибок в консоли. (Тест в Task 3.)
2. Полка пустая — показывается подсказка «Перетащите файл сюда», плитка «В Telegram» не видна. (Тест в Task 3.)
3. Быстрое переключение вкладок (Alt+1..4 подряд) — не остаётся двух разделов наложенными, направление считается от последней вкладки. (Тест `deckDirection` в Task 1.)
4. Стрик без данных (GitHub не подключён / 0 вкладов) — count-up до 0 без деления на ноль, волна не стартует по пустой сетке. (Тест `countUpValue` в Task 1.)
5. Длинные имена файлов, кириллица, имена без расширения — усечение по середине сохраняет расширение. (Тест `middleTruncate` в Task 1.)

---

### Task 1: Чистые функции анимаций + vitest

**Files:**
- Create: `src/island/motion.ts`
- Create: `src/island/motion.test.ts`
- Modify: `package.json` (devDependency `vitest`, script `"test": "vitest run"`)

**Interfaces:**
- Produces:
  - `deckDirection(from: PeekDeck, to: PeekDeck): 1 | -1 | 0` — по порядку `["nook","focus","tray","activity"]`.
  - `deckVariants: Variants` — `enter(dir)`: `{x: 24*dir, opacity: 0, filter: "blur(6px)"}`, `center`: `{x:0, opacity:1, filter:"blur(0px)"}`, `exit(dir)`: `{x: -24*dir, opacity:0, filter:"blur(6px)"}`.
  - `DECK_SPRING = { type: "spring", stiffness: 420, damping: 38 }`.
  - `middleTruncate(name: string, max: number): string`.
  - `waveDelay(weekIndex: number, totalWeeks: number): number` — секунды, `min(weekIndex*0.012, 0.65)`.
  - `countUpValue(target: number, t: number): number` — `t∈[0,1]`, easeOutCubic, целое, `t>=1 → target`.
  - `FOCUS_PRESETS = [5, 15, 25, 50] as const`.

- [ ] **Step 1: Write the failing tests** (`motion.test.ts`)
  - `deckDirection("nook","tray") === 1`, `("activity","focus") === -1`, `("tray","tray") === 0`.
  - `middleTruncate("Bartender Materials", 16) === "Barten…aterials"` — длина ≤ 16, есть `…`.
  - `middleTruncate("Отчёт_за_ноябрь_финальный.pdf", 14)` заканчивается на `".pdf"`, длина ≤ 14.
  - `middleTruncate("a.txt", 16) === "a.txt"`; `middleTruncate("README", 4).length <= 4`.
  - `waveDelay(0, 53) === 0`; `waveDelay(10, 53) ≈ 0.12`; `waveDelay(200, 53) === 0.65`.
  - `countUpValue(727, 0) === 0`, `countUpValue(727, 1) === 727`, `countUpValue(0, 0.5) === 0`, `countUpValue(727, 0.5) > 363` (easeOut обгоняет линию).
- [ ] **Step 2:** `npm i -D vitest` и `npx vitest run src/island/motion.test.ts` → FAIL (модуль не найден).
- [ ] **Step 3:** Реализовать `motion.ts`. `middleTruncate`: если есть расширение ≤ 5 символов — сохранять `….ext` в хвосте, остаток делить ~поровну.
- [ ] **Step 4:** `npx vitest run` → PASS; `npm run typecheck` → без ошибок.
- [ ] **Step 5: Commit** — `git add src/island/motion.ts src/island/motion.test.ts package.json package-lock.json`, `feat(island): motion helpers for deck transitions`.

### Task 2: Переход между разделами с направлением

**Files:**
- Modify: `src/island/views/PeekView.tsx` (контейнер раздела)
- Modify: `src/store/island.ts` (хранить `prevDeck` при `setDeck`)

**Interfaces:**
- Consumes: `deckDirection`, `deckVariants`, `DECK_SPRING` (Task 1).
- Produces: `useIsland` поле `deckDir: 1 | -1 | 0`, обновляется в `setDeck` до смены `peekDeck`.

- [ ] **Step 1:** В `setDeck(to)` вычислять `deckDir = deckDirection(peekDeck, to)`; повторный вызов с той же вкладкой ничего не меняет.
- [ ] **Step 2:** Тело раздела обернуть в `<AnimatePresence mode="popLayout" custom={deckDir} initial={false}>` с `motion.div key={peekDeck} data-deck={peekDeck} variants={deckVariants} custom={deckDir} initial="enter" animate="center" exit="exit" transition={DECK_SPRING}`. При `useReducedMotion()` — варианты только с opacity.
- [ ] **Step 3:** Проверка в стенде (`npx vite` → `/qa/notch-showcase.html`): переключить 1→3→2 и быстро Alt+1..4; в DOM одновременно максимум 2 раздела в процессе перехода и 1 через 400 мс (`document.querySelectorAll('[data-deck]').length`). Скриншот середины перехода.
- [ ] **Step 4:** `npm run typecheck` → OK.
- [ ] **Step 5: Commit** — только два файла, `feat(island): directional deck transitions`.

### Task 3: Полка плитками + миниатюры

**Files:**
- Modify: `src-tauri/src/filekit/mod.rs` (`fn info_of` → `pub(crate)`)
- Modify: `src-tauri/src/ipc_hub.rs` (+ команда), `src-tauri/src/lib.rs` (регистрация)
- Modify: `src/lib/ipc.ts` (`ipc.hub.shelfThumbnail`)
- Modify: `src/island/views/TrayDeck.tsx`, `src/island/island.css`
- Modify: `src/qa-notch.tsx` (демо-плитки с data-URL превью, один элемент `exists:false`)

**Interfaces:**
- Produces (Rust): `#[tauri::command] pub async fn hub_shelf_thumbnail(path: String) -> AppResult<Option<String>>` — `filekit::info_of(path).thumbnail` в `platform::blocking`; несуществующий путь → `Ok(None)`.
- Produces (TS): `ipc.hub.shelfThumbnail(path: string): Promise<string | null>`; в `TrayDeck` кэш `Map<path, string|null>` на время жизни окна.
- Produces: компонент `ShelfTile({ item, thumb, onOpen })`, корень с `data-shelf-tile`, `data-exists`.

- [ ] **Step 1: Rust test** в `filekit/mod.rs` `#[cfg(test)]`: `shelf_thumbnail_for_png_is_data_url` — записать 4×4 PNG через `image` в temp-dir, `info_of(&p, false).thumbnail` начинается с `"data:image/"`; `missing_file_has_no_thumbnail` — для несуществующего пути команда-обёртка возвращает `None` (вынести логику в `pub(crate) fn shelf_thumbnail(path:&Path)->Option<String>`).
- [ ] **Step 2:** `cargo test --lib shelf_thumbnail` (из `src-tauri`) → FAIL.
- [ ] **Step 3:** Реализовать `shelf_thumbnail` + команду + регистрацию + `ipc.ts`.
- [ ] **Step 4:** `cargo test --lib shelf_thumbnail` → PASS.
- [ ] **Step 5:** Переписать `TrayDeck`: ряд плиток 96×96, подпись `middleTruncate(label, 16)`, ввод текста/ссылки над рядом, горизонтальный скролл колесом. Файл без превью/папка/текст/ссылка → иконка типа. Наведение: `scale 1.04` + до 3 кнопок («В чат», «Показать в папке», «Убрать»). Новый элемент: `motion` `layout`, `initial={{scale:0.6, opacity:0}}`. `exists:false` → `opacity .45`, кнопки отправки/drag отключены. Пустая полка → «Перетащите файл сюда», без плитки Telegram.
- [ ] **Step 6:** Стенд: скриншот Полки с 4 плитками; проверить Review Focus 1–2 и 5 (`[data-exists="false"]` без кнопки drag; пустое состояние).
- [ ] **Step 7:** `npm run typecheck`, `npx vitest run` → OK.
- [ ] **Step 8: Commit** — файлы задачи, `feat(island): shelf as thumbnail tiles`.

### Task 4: Плитка «В Telegram» и drag-out

**Files:**
- Create: `src-tauri/src/platform/dragout.rs`
- Modify: `src-tauri/src/platform/mod.rs`, `src-tauri/src/ipc_hub.rs`, `src-tauri/src/lib.rs`, `src/lib/ipc.ts`
- Modify: `src/island/views/TrayDeck.tsx`, `src/island/island.css`

**Interfaces:**
- Produces (Rust): `pub fn drag_files_out(hwnd: HWND, paths: &[String]) -> AppResult<bool>` (true = цель приняла). Реализация: `SHParseDisplayName` для каждого пути → `SHCreateShellItemArrayFromIDLists` → `BindToHandler(BHID_DataObject)` → `SHDoDragDrop(hwnd, &data, None, DROPEFFECT_COPY)`. Вызывать через `app.run_on_main_thread` (STA-поток WebView).
- Produces (Rust): `#[tauri::command] pub async fn hub_shelf_drag_out(window: WebviewWindow, paths: Vec<String>) -> AppResult<bool>`; пустой список или несуществующие пути → `AppError::invalid`.
- Produces (TS): `ipc.hub.shelfDragOut(paths: string[]): Promise<boolean>`.
- Consumes: существующий `invoke("files_action", { paths, action: "telegram" })` и событие `files:job` (результат).

- [ ] **Step 1: Rust test** `drag_out_rejects_missing_paths` — `validate_drag_paths(&["C:/nope/x.txt"])` → `Err`; `validate_drag_paths(&[])` → `Err`; существующий temp-файл → `Ok`. (Выделить `validate_drag_paths` как чистую функцию.)
- [ ] **Step 2:** `cargo test --lib drag_out` → FAIL; реализовать; → PASS.
- [ ] **Step 3:** В `ShelfTile`: `pointerdown` + сдвиг > 4 px → `ipc.hub.shelfDragOut([item.value])` (только `kind==="file" && exists`).
- [ ] **Step 4:** Плитка «В Telegram» справа (как AirDrop на референсе): иконка + «В Telegram», подпись «Избранное». Клик — отправить выбранную плитку (последнюю нажатую; если не выбрана — кнопка неактивна). Drop файла с Проводника на неё (по координатам Tauri drag-drop в `Island.tsx`, ветка `peekDeck === "tray"`) — отправить эти пути. Наведение с файлом: янтарный контур + один пульс иконки. Успех из `files:job` → галочка 1,3 с; ошибка → текст ошибки в подписи.
- [ ] **Step 5:** `npm run typecheck` → OK; `cargo build` → OK.
- [ ] **Step 6: Живая проверка** в dev-приложении (`npx tauri dev`): вытащить плитку в Проводник — файл скопирован (проверить `Test-Path`); отправить в Telegram — в заголовке окна Telegram «Избранное», сообщение с файлом есть. Что не удалось проверить — записать явно.
- [ ] **Step 7: Commit** — файлы задачи, `feat(island): drag files out of the shelf and send to Telegram`.

### Task 5: Стрик — волна, вспышка, count-up

**Files:**
- Modify: `src/island/views/ActivityDeck.tsx`, `src/island/island.css`

**Interfaces:**
- Consumes: `waveDelay`, `countUpValue` (Task 1).
- Produces: хук `useCountUp(target: number, ms = 700): number` в `ActivityDeck.tsx` (rAF только на время анимации, затем остановка).

- [ ] **Step 1:** Клетки: `motion.span` с `initial={{opacity:0, scale:0.6}} animate={{opacity:1, scale:1}} transition={{delay: waveDelay(week, weeks), duration: 0.18}}`; анимация только при монтировании раздела (ключ = момент открытия, не данные). Пустая сетка — без волны.
- [ ] **Step 2:** Сегодняшняя клетка: после `waveDelay(lastWeek)+0.2` одна вспышка `box-shadow 0 → 0 0 8px var(--amber) → 0`, 600 мс, `animation-iteration-count: 1`.
- [ ] **Step 3:** «N вкладов за год», «Серия», «Рекорд» через `useCountUp`.
- [ ] **Step 4:** Reduced motion → без задержек и count-up.
- [ ] **Step 5:** Стенд: кадры 0, 300, 900 мс после открытия; через 1,5 с `document.getAnimations().length === 0`.
- [ ] **Step 6:** `npm run typecheck` → OK. **Commit** — `feat(island): streak wave and count-up`.

### Task 6: Фокус — крупное кольцо, пресеты, flip

**Files:**
- Modify: `src/island/views/FocusDeck.tsx`, `src/island/island.css`

**Interfaces:**
- Consumes: `FOCUS_PRESETS` (Task 1), существующие `FocusRingSvg`, `focusStart(title, minutes, taskId)`.
- Produces: компонент `FlipNumber({ value }: { value: number })`.

- [ ] **Step 1:** Левая колонка: `FocusRingSvg size={96}`; дуга обновляется раз в секунду, плавность — CSS `transition: stroke-dashoffset 1s linear` (без rAF).
- [ ] **Step 2:** Под кольцом пилюли `FOCUS_PRESETS` (выбранная — bone), заменяют текущий выбор `presetMin`; во время активного фокуса пилюли скрыты, видны «Пауза»/«Готово».
- [ ] **Step 3:** `FlipNumber`: `AnimatePresence` по `value`, старое `y:0→-10, blur 0→4px, opacity→0`, новое снизу, 220 мс; reduced motion — без сдвига.
- [ ] **Step 4:** Стенд: скриншот Фокуса в покое и в активном фокусе; через 1,5 с после смены минуты `getAnimations()` содержит только transition кольца.
- [ ] **Step 5:** `npm run typecheck` → OK. **Commit** — `feat(island): focus ring, presets and flip digits`.

### Task 7: Итоговая проверка

- [ ] `npx vitest run`, `npm run typecheck`, `cargo test --lib` (из `src-tauri`) — всё зелёное.
- [ ] Скриншоты каждого раздела + кадры середины перехода из стенда — показать пользователю.
- [ ] Сводка: что проверено вживую (drag-out, Telegram), что нет.
