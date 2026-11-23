// The uninstaller's screens. Rust drives progress through `window.uninstall`.
(function () {
  const post = (m) => window.ipc && window.ipc.postMessage(m);
  const $ = (id) => document.getElementById(id);
  const config = window.UNINSTALL || { version: "", found: true };
  const primary = $("primary");
  const closeBtn = $("close-btn");
  const cache = $("opt-cache");
  const personal = $("opt-personal");
  let state = "";
  let shown = 0;
  let target = 0;
  let sizes = null;

  $("version").textContent = config.version ? `Версия ${config.version}` : "";

  function show(id) {
    document.querySelectorAll(".screen").forEach((s) => s.classList.toggle("is-on", s.id === id));
    state = id;
  }

  function size(bytes) {
    const gb = bytes / 1024 ** 3;
    if (gb >= 1) return `${gb.toFixed(1).replace(".", ",")} ГБ`;
    const mb = bytes / 1024 ** 2;
    return mb >= 1 ? `${Math.round(mb)} МБ` : "меньше мегабайта";
  }

  function tick() {
    if (shown < target) {
      shown = Math.min(target, shown + Math.max(1, Math.round((target - shown) / 6)));
      $("pct").textContent = String(shown);
    }
    requestAnimationFrame(tick);
  }
  requestAnimationFrame(tick);

  function step(n) {
    document.querySelectorAll(".step").forEach((li) => {
      const i = Number(li.dataset.step);
      li.classList.toggle("is-done", i < n);
      li.classList.toggle("is-run", i === n);
    });
  }

  // Settings and keys live in the same folder as the models, so removing
  // them removes everything.
  function syncOptions() {
    if (personal.checked) cache.checked = true;
    cache.disabled = personal.checked;
  }
  personal.addEventListener("change", syncOptions);

  function footer(primaryText, closeText) {
    primary.textContent = primaryText;
    primary.disabled = false;
    primary.hidden = !primaryText;
    closeBtn.textContent = closeText;
    closeBtn.hidden = !closeText;
  }

  window.uninstall = {
    sizes(cacheBytes, allBytes) {
      sizes = { cache: cacheBytes, all: allBytes };
      $("cache-note").textContent =
        cacheBytes > 0
          ? `Освободит ${size(cacheBytes)}. Если вернётесь, они скачаются заново`
          : "Их пока нет — удалять нечего";
    },
    progress(p) {
      target = Math.max(target, Math.min(100, Math.round(p)));
    },
    step,
    done(freed, left) {
      target = 100;
      step(3);
      const kept = personal.checked
        ? "Настройки, история и ключи API удалены."
        : "Настройки и ключи API сохранены — после переустановки всё вернётся.";
      $("done-lead").textContent = freed > 0 ? `Освобождено ${size(freed)}. ${kept}` : kept;
      $("left").hidden = !left;
      $("left").textContent = left ? `Папка ${left} занята и осталась на диске. Её можно удалить вручную после перезагрузки.` : "";
      setTimeout(() => {
        show("done");
        footer("Готово", "");
      }, 650);
    },
    failed(message) {
      $("error").textContent = message;
      show("failed");
      footer("Повторить", "Закрыть");
    },
  };

  function start() {
    const what = personal.checked ? "all" : cache.checked ? "cache" : "none";
    $("data-title").textContent = what === "none" ? "Наводим порядок" : "Освобождаем место";
    $("data-note").textContent =
      what === "all" ? "Удаляем настройки, модели и ключи…" : what === "cache" ? "Удаляем модели и кэш…" : "Проверяем, что ничего не осталось…";
    show("progress");
    target = 0;
    shown = 0;
    $("pct").textContent = "0";
    step(0);
    primary.disabled = true;
    primary.textContent = "Удаляю…";
    closeBtn.hidden = true;
    post(`uninstall:${cache.checked ? 1 : 0}${personal.checked ? 1 : 0}`);
  }

  primary.addEventListener("click", () => {
    if (state === "confirm" || state === "failed") start();
    else if (state === "done" || state === "missing") post("close");
  });

  document.addEventListener(
    "click",
    (e) => {
      const t = e.target.closest("[data-cmd],[data-open]");
      if (!t) return;
      // The window stays open until the removal has finished.
      if (t.dataset.cmd === "close" && state === "progress") return;
      if (t.dataset.cmd) post(t.dataset.cmd);
      if (t.dataset.open) post(`open:${t.dataset.open}`);
    },
    true,
  );
  document.addEventListener("mousedown", (e) => {
    if (e.button === 0 && e.target.closest("[data-drag]") && !e.target.closest("button")) post("drag");
  });
  document.addEventListener("contextmenu", (e) => e.preventDefault());
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && state !== "progress") post("close");
    if (e.key === "Enter" && !primary.disabled && !primary.hidden) primary.click();
  });

  if (config.found) {
    show("confirm");
    footer("Удалить", "Отмена");
  } else {
    show("missing");
    footer("Закрыть", "");
  }

  const ready = () =>
    requestAnimationFrame(() => {
      post("ready");
      if (config.found) post("measure");
    });
  if (document.readyState === "complete") ready();
  else window.addEventListener("load", ready);
})();
