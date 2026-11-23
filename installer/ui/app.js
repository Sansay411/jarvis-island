// The installer's three screens. Rust drives progress through `window.setup`.
(function () {
  const post = (m) => window.ipc && window.ipc.postMessage(m);
  const $ = (id) => document.getElementById(id);
  const primary = $("primary");
  const closeBtn = $("close-btn");
  let state = "welcome";
  let shown = 0;
  let target = 0;
  let installed = "";

  $("version").textContent = `Версия ${(window.SETUP && window.SETUP.version) || ""}`;

  function show(id) {
    document.querySelectorAll(".screen").forEach((s) => s.classList.toggle("is-on", s.id === id));
    state = id;
  }

  // The number counts up smoothly towards what the installer reported.
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

  window.setup = {
    progress(p) {
      target = Math.max(target, Math.min(100, Math.round(p)));
      if (target >= 6 && document.querySelector('.step[data-step="0"]').classList.contains("is-run")) step(1);
    },
    step,
    done(path) {
      installed = path;
      target = 100;
      step(3);
      setTimeout(() => {
        show("done");
        primary.textContent = "Запустить Джарвиса";
        primary.disabled = false;
        closeBtn.hidden = false;
      }, 650);
    },
    failed(message) {
      $("error").textContent = message;
      show("failed");
      primary.textContent = "Повторить";
      primary.disabled = false;
      closeBtn.hidden = false;
    },
  };

  primary.addEventListener("click", () => {
    if (state === "welcome" || state === "failed") {
      show("progress");
      target = 0;
      shown = 0;
      step(0);
      primary.disabled = true;
      primary.textContent = "Устанавливаю…";
      closeBtn.hidden = true;
      post(`install:${$("desktop").checked ? 1 : 0}`);
    } else if (state === "done") {
      post(`launch:${installed}`);
    }
  });

  document.addEventListener("click", (e) => {
    const t = e.target.closest("[data-cmd],[data-open]");
    if (!t) return;
    if (t.dataset.cmd) post(t.dataset.cmd);
    if (t.dataset.open) post(`open:${t.dataset.open}`);
  });
  document.addEventListener("mousedown", (e) => {
    if (e.button === 0 && e.target.closest("[data-drag]") && !e.target.closest("button")) post("drag");
  });
  document.addEventListener("contextmenu", (e) => e.preventDefault());
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && state !== "progress") post("close");
    if (e.key === "Enter" && !primary.disabled) primary.click();
  });

  const ready = () => requestAnimationFrame(() => post("ready"));
  if (document.readyState === "complete") ready();
  else window.addEventListener("load", ready);
})();
