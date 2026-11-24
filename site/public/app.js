"use strict";

const CFG = window.JARVIS || {};
const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => [...el.querySelectorAll(s)];
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;

/* ---------------- i18n ---------------- */

const EN = {
  "nav.features": "Features",
  "nav.tour": "The island",
  "nav.pricing": "Pricing",
  "nav.faq": "FAQ",
  "nav.download": "Download",
  "hero.eyebrow": "AI agent for Windows and macOS",
  "hero.title": 'Say “Jarvis”.<br><span class="amber">Your computer does the rest.</span>',
  "hero.lead": "A voice assistant that lives in an island at the top of your screen. It sets the volume, opens apps, tiles windows, runs your calendar, and checks every step before it answers.",
  "cta.windows": "Download for Windows",
  "cta.mac": "Download for macOS",
  "cta.tour": "See it in action",
  "hero.fine": "Free · No keys, no sign-up · Windows 10/11 and macOS",
  "stat.cpu": "CPU when idle",
  "stat.wake": "from its name to a reaction",
  "stat.local": "volume change, no model involved",
  "stat.electron": "Electron or Python. Pure Rust",
  "tour.kicker": "The island",
  "tour.title": "One island. Everything at hand.",
  "tour.lead": "Hover and the island opens: music with your calendar, focus, growth, the shelf and sound. These are real screens from the app.",
  "tour.voice": "Voice",
  "tour.nook": "Overview",
  "tour.focus": "Focus",
  "tour.growth": "Growth",
  "tour.tray": "Shelf",
  "tour.sound": "Sound",
  "tour.files": "Files",
  "tour.voice.alt": "A spoken answer on the island",
  "tour.nook.alt": "Overview: player and calendar",
  "tour.focus.alt": "Focus for a calendar block",
  "tour.growth.alt": "Growth: streak, level, short video minutes",
  "tour.tray.alt": "The shelf with files",
  "tour.sound.alt": "Sound: earbud battery",
  "tour.files.alt": "What to do with a file",
  "how.kicker": "How it works",
  "how.title": "One sentence instead of ten clicks",
  "how.1.t": "Say it",
  "how.1.p": "“Jarvis, set the volume to 30 and open my last project.” Its name is heard by a model on your computer: no sound leaves it until you call.",
  "how.2.t": "It does it",
  "how.2.p": "It doesn’t advise, it clicks: native Windows and macOS APIs, windows, the browser, files. Long tasks become a live plan right on the island.",
  "how.3.t": "And checks",
  "how.3.p": "After every action it looks at what really happened, and only then answers out loud. Anything risky waits for your “Allow”.",
  "feat.kicker": "Features",
  "feat.title": "Not a chatbot. Hands for your computer.",
  "feat.voice.t": "A voice you never wait for",
  "feat.voice.p": "On-device wake word, studio neural voices without keys, and an offline voice when the internet drops. Interrupt it mid-sentence and it stops.",
  "feat.voice.alt1": "Jarvis looking for an answer",
  "feat.voice.alt2": "Jarvis answering out loud",
  "feat.act.t": "Many steps, one request",
  "feat.act.p": "Volume, brightness, apps, 50/50 windows, browser tabs, timers. A live plan and a check mark on every step.",
  "feat.act.alt": "Chat with a plan and finished actions",
  "feat.files.t": "Drop a file on the island",
  "feat.files.p": "MKV to MP4, audio from video, text from images and PDFs, subtitles from audio, ZIP, EXIF removal.",
  "feat.files.alt1": "Dropping a file on the island",
  "feat.files.alt2": "File actions",
  "feat.plan.t": "A calendar like Google’s",
  "feat.plan.p": "Week, day and month views, two-way Google sync and “plan my day”.",
  "feat.plan.alt": "The planner, a week view",
  "feat.focus.t": "Focus by the calendar",
  "feat.focus.p": "Ten minutes before a block the island offers a focus session. Your streak and level grow from real work.",
  "feat.focus.alt": "A calendar block reminder",
  "feat.mentor.t": "A mentor instead of the feed",
  "feat.mentor.p": "A limit for Shorts, Reels and TikTok. When the minutes run out the tab closes and you get coding and English cards, a stretch, or a step on your project.",
  "feat.clip.t": "Clipboard memory",
  "feat.clip.p": "“Paste what I copied an hour ago.” Passwords and keys are never kept, everything stays on your computer.",
  "feat.tg.t": "Your own Telegram bot",
  "feat.tg.p": "A morning plan, an evening summary, reminders and requests to Jarvis from your phone. No third-party server.",
  "feat.dev.t": "For developers",
  "feat.dev.p": "VS Code and recent projects, Claude Code and Codex as background jobs, the terminal, any MCP server.",
  "feat.models": "Any model: works out of the box through Jarvis Cloud, or with your own key",
  "feat.models.any": "any OpenAI-compatible API",
  "priv.kicker": "Privacy and safety",
  "priv.title": "It runs a real computer. So it is careful.",
  "priv.1": "<b>The name is heard locally.</b> Until you call Jarvis, neither sound nor text leaves your computer.",
  "priv.2": "<b>Risky actions need your “Allow”.</b> Deleting, installing, sending messages and buying, even on autopilot.",
  "priv.3": "<b>Other people’s text gives no orders.</b> Pages, files and the clipboard are untrusted data to the model.",
  "priv.4": "<b>No accounts, no telemetry.</b> Keys live in the Windows credential store and the macOS keychain; risky actions go to a local log.",
  "priv.alt": "Confirming a risky action",
  "road.kicker": "Where we’re going",
  "road.title": "The goal: a computer that’s easier to run by voice than with a mouse",
  "road.now": "Works today",
  "road.now.p": "The island, Russian voice, 40+ system actions, calendar, mentor, Telegram, Windows and macOS.",
  "road.next": "Next",
  "road.next.p": "English and Kazakh voice, a name of your choice, notes and reliable reminders, “undo the last thing”.",
  "road.later": "Later",
  "road.later.p": "Any app by voice with no buttons, routines it learns on its own, your phone as the computer’s microphone.",
  "price.kicker": "Pricing",
  "price.title": "Free forever. Pro when you want more.",
  "price.monthly": "Monthly",
  "price.yearly": "Yearly",
  "price.save": "-30%",
  "price.free.sub": "The whole app, no card, no sign-up",
  "price.free.1": "Every island, voice and agent feature",
  "price.free.2": "Jarvis Cloud: 300 requests and 300 voice replies a day",
  "price.free.3": "20 minutes of cloud speech recognition a day",
  "price.free.4": "Your own key or Ollama: no limits",
  "price.free.5": "Offline voice and on-device recognition",
  "price.free.cta": "Download free",
  "price.pro.tag": "For people who talk a lot",
  "price.pro.sub": "Cancel in one click",
  "price.per.month": "/mo",
  "price.per.year": "/yr",
  "price.pro.1": "10× higher cloud limits",
  "price.pro.2": "A stronger model for long, hard tasks",
  "price.pro.3": "Premium cloud voices",
  "price.pro.4": "New features first",
  "price.pro.5": "Support straight from the developer",
  "price.pro.cta": "Get Pro",
  "price.fine": "Prices in USD; taxes are handled by our payment partner. The money pays for the cloud and the project.",
  "faq.kicker": "FAQ",
  "faq.title": "The short version",
  "faq.1.q": "Is it really free?",
  "faq.1.a": "Yes. Free is the whole app. The default cloud has a daily limit, and with your own model key or a local Ollama there is no limit at all. Pro is for people who talk to Jarvis through the cloud a lot.",
  "faq.2.q": "Do I need API keys?",
  "faq.2.a": "No. Right after install it works through Jarvis Cloud: model, voice and speech recognition without a single key. You can add your own key in settings any time.",
  "faq.3.q": "Does it work offline?",
  "faq.3.a": "The wake word, speech recognition and voice can all run on your computer. For smart answers without a network, connect a local model through Ollama. Simple commands like volume, music and brightness need no model at all.",
  "faq.4.q": "Which languages does it understand?",
  "faq.4.a": "By voice, Russian for now. In chat, any language your model speaks. English and Kazakh voice are next on the roadmap.",
  "faq.5.q": "Will it do something I didn’t want?",
  "faq.5.a": "Reading happens right away, changes ask first, and deleting, installing, sending and buying always wait for your “Allow”, even on autopilot. Every risky action is written to a local log.",
  "faq.6.q": "Windows warns about an unknown publisher",
  "faq.6.a": "Builds aren’t code-signed yet: signing through SignPath Foundation is on its way. Until then click “More info” → “Run anyway”. The source code is open on GitHub.",
  "faq.7.q": "How do I cancel Pro?",
  "faq.7.a": "Your payment e-mail has a link to the customer portal: cancel in one click and Pro keeps working until the end of the paid period. Free stays with you.",
  "final.title": "Call it by its name",
  "final.lead": "A one-minute install, no admin rights. It updates itself after that.",
  "dl.win": "10 and 11, x64",
  "dl.arm": "Apple Silicon",
  "dl.intel": "Intel",
  "dl.portable": "Portable version for Windows",
  "dl.all": "All releases",
  "foot.privacy": "Privacy",
  "foot.changelog": "What’s new",
  "wait.title": "Pro opens soon",
  "wait.lead": "Leave your e-mail and we’ll write once, when Pro can be bought.",
  "wait.cta": "Tell me",
};

const META = {
  ru: {
    title: document.title,
    stats: ["0%", "~0,45 c", "36 мс", "0"],
    yearNote: (m) => `$${m}/мес при оплате за год`,
    monthNote: "Отмена в один клик",
    sent: "Готово! Напишем, как только Pro откроется.",
    bad: "Проверьте адрес почты.",
    fail: "Не получилось отправить. Попробуйте ещё раз.",
  },
  en: {
    title: "Jarvis: say “Jarvis”, your computer does the rest",
    stats: ["0%", "~0.45 s", "36 ms", "0"],
    yearNote: (m) => `$${m}/mo billed yearly`,
    monthNote: "Cancel in one click",
    sent: "Done! We’ll write as soon as Pro opens.",
    bad: "Please check the e-mail address.",
    fail: "Couldn’t send it. Please try again.",
  },
};

const RU = {};
for (const el of $$("[data-i18n]")) RU[el.dataset.i18n] = el.innerHTML;
const RU_ALT = {};
for (const el of $$("[data-i18n-alt]")) RU_ALT[el.dataset.i18nAlt] = el.alt;

function pickLang() {
  const q = new URLSearchParams(location.search).get("lang");
  if (q === "ru" || q === "en") return q;
  const saved = localStorage.getItem("lang");
  if (saved === "ru" || saved === "en") return saved;
  return /^(ru|kk|uk|be|ky|uz)/i.test(navigator.language || "") ? "ru" : "en";
}

let lang = pickLang();

function applyLang() {
  const dict = lang === "en" ? EN : RU;
  for (const el of $$("[data-i18n]")) {
    const v = dict[el.dataset.i18n];
    if (v !== undefined) el.innerHTML = v;
  }
  for (const el of $$("[data-i18n-alt]")) {
    el.alt = lang === "en" ? EN[el.dataset.i18nAlt] || el.alt : RU_ALT[el.dataset.i18nAlt];
  }
  document.documentElement.lang = lang;
  document.title = META[lang].title;
  $("#lang").textContent = lang === "en" ? "RU" : "EN";
  $$(".stat b").forEach((b, i) => (b.textContent = META[lang].stats[i]));
  renderMarquee();
  renderPrice();
  setupDownloads();
}

$("#lang").addEventListener("click", () => {
  lang = lang === "en" ? "ru" : "en";
  localStorage.setItem("lang", lang);
  applyLang();
  demo.restart();
});

/* ---------------- downloads ---------------- */

let platform = "windows";
const ua = navigator.userAgent;
if (/Mac/i.test(navigator.platform || ua) && !/iPhone|iPad/i.test(ua)) platform = "mac-arm";
if (navigator.userAgentData?.getHighEntropyValues && platform !== "windows") {
  navigator.userAgentData
    .getHighEntropyValues(["architecture"])
    .then((v) => {
      if (v.architecture === "x86") platform = "mac-intel";
      setupDownloads();
    })
    .catch(() => {});
}

function setupDownloads() {
  const mac = platform !== "windows";
  for (const a of $$("[data-download]")) a.href = `/download/${platform}`;
  for (const s of $$("[data-download-label]")) {
    s.innerHTML = mac ? (lang === "en" ? EN["cta.mac"] : "Скачать для macOS") : lang === "en" ? EN["cta.windows"] : RU["cta.windows"];
  }
  for (const d of $$(".dl")) d.classList.toggle("mine", d.dataset.platform === platform);
}

/* ---------------- marquee of things to say ---------------- */

const SAY = {
  ru: [
    "Джарвис, громкость 30", "открой последний проект", "разложи окна пополам", "что играет?", "поставь фокус на 25 минут",
    "напомни в 18:30 позвонить маме", "спланируй мне завтра", "сделай ярче", "закрой лишние окна", "переведи это видео в MP4",
    "вставь то, что я копировал час назад", "найди в интернете и открой", "сколько заряда в наушниках?", "давай разминку",
  ],
  en: [
    "Jarvis, volume 30", "open my last project", "tile these windows", "what’s playing?", "focus for 25 minutes",
    "remind me at 6:30 to call mom", "plan my tomorrow", "brighter", "close the clutter", "turn this video into MP4",
    "paste what I copied an hour ago", "search it and open it", "how much charge in my earbuds?", "let’s stretch",
  ],
};

function renderMarquee() {
  const items = SAY[lang];
  $("#sayTrack").innerHTML = [...items, ...items].map((t) => `<span>${t}</span>`).join("");
}

/* ---------------- pricing ---------------- */

let period = "monthly";
const price = CFG.price || { monthly: 7, yearly: 59 };

function renderPrice() {
  const yearly = period === "yearly";
  $("#proPrice").textContent = `$${yearly ? price.yearly : price.monthly}`;
  const per = $("#proPer");
  per.dataset.i18n = yearly ? "price.per.year" : "price.per.month";
  per.textContent = lang === "en" ? EN[per.dataset.i18n] : yearly ? "/год" : "/мес";
  const monthOfYear = Math.round((price.yearly / 12) * 100) / 100;
  $("#proNote").textContent = yearly ? META[lang].yearNote(monthOfYear.toFixed(2)) : META[lang].monthNote;
  const save = Math.round((1 - price.yearly / (price.monthly * 12)) * 100);
  $$(".period em").forEach((e) => (e.textContent = `-${save}%`));
}

for (const b of $$("[data-period]")) {
  b.addEventListener("click", () => {
    period = b.dataset.period;
    $$("[data-period]").forEach((x) => x.classList.toggle("on", x === b));
    renderPrice();
  });
}

/* ---------------- checkout or waitlist ---------------- */

let paddleReady = null;
function loadPaddle() {
  paddleReady ??= new Promise((resolve, reject) => {
    const s = document.createElement("script");
    s.src = "https://cdn.paddle.com/paddle/v2/paddle.js";
    s.onload = () => {
      const { token } = CFG.paddle;
      if (token.startsWith("test_")) window.Paddle.Environment.set("sandbox");
      window.Paddle.Initialize({ token, checkout: { settings: { displayMode: "overlay", theme: "dark" } } });
      resolve(window.Paddle);
    };
    s.onerror = reject;
    document.head.append(s);
  });
  return paddleReady;
}

$("#buyPro").addEventListener("click", async () => {
  const priceId = CFG.paddle?.prices?.[period];
  if (CFG.paddle?.token && priceId) {
    try {
      const Paddle = await loadPaddle();
      Paddle.Checkout.open({ items: [{ priceId, quantity: 1 }], settings: { locale: lang } });
      return;
    } catch {
      /* fall through to the waitlist */
    }
  }
  $("#waitMsg").textContent = "";
  $("#waitModal").showModal();
});

$("#waitForm").addEventListener("submit", async (e) => {
  e.preventDefault();
  const form = e.currentTarget;
  const msg = $("#waitMsg");
  const btn = $("button[type=submit]", form);
  btn.disabled = true;
  msg.className = "wait-msg";
  try {
    const res = await fetch("/api/waitlist", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ email: form.email.value, website: form.website.value, plan: period, lang }),
    });
    const body = await res.json().catch(() => ({}));
    if (res.ok && body.ok) {
      msg.textContent = META[lang].sent;
      msg.classList.add("ok");
      form.reset();
    } else {
      msg.textContent = body.error === "email" ? META[lang].bad : META[lang].fail;
      msg.classList.add("err");
    }
  } catch {
    msg.textContent = META[lang].fail;
    msg.classList.add("err");
  } finally {
    btn.disabled = false;
  }
});

/* ---------------- modals ---------------- */

for (const d of $$("dialog")) {
  d.addEventListener("click", (e) => {
    if (e.target === d || e.target.closest("[data-close]")) d.close();
  });
}

/* ---------------- the island tour and cycling screens ---------------- */

const TAB_MS = 3800;
const tourShots = $$("#tourScreen .shot");
const tabs = $$(".tabs button");
let tourAt = 0;
let tourTimer = 0;
function showTab(i, user) {
  tourAt = (i + tabs.length) % tabs.length;
  const id = tabs[tourAt].dataset.tab;
  tabs.forEach((t) => t.classList.toggle("on", t.dataset.tab === id));
  tourShots.forEach((s) => s.classList.toggle("is-on", s.dataset.tab === id));
  // Restart the progress bar on the active tab.
  const on = tabs[tourAt];
  on.classList.remove("on");
  void on.offsetWidth;
  on.classList.add("on");
  clearTimeout(tourTimer);
  if (!reduced) tourTimer = setTimeout(() => showTab(tourAt + 1), user ? TAB_MS * 2 : TAB_MS);
  $(".tabs").style.setProperty("--tab-ms", `${user ? TAB_MS * 2 : TAB_MS}ms`);
}
tabs.forEach((t, i) => t.addEventListener("click", () => showTab(i, true)));

for (const screen of $$("[data-cycle]")) {
  const shots = $$(".shot", screen);
  let at = 0;
  setInterval(() => {
    if (document.hidden) return;
    shots[at].classList.remove("is-on");
    at = (at + 1) % shots.length;
    shots[at].classList.add("is-on");
  }, 2600 + Math.random() * 600);
}

// The tour runs only while it is on screen.
new IntersectionObserver(([e]) => {
  if (e.isIntersecting) showTab(tourAt);
  else clearTimeout(tourTimer);
}).observe($("#tourScreen"));

/* ---------------- 3D tilt toward the pointer ---------------- */

if (!reduced && matchMedia("(pointer: fine)").matches) {
  for (const el of $$(".tilt")) {
    el.addEventListener("pointermove", (e) => {
      const r = el.getBoundingClientRect();
      const x = (e.clientX - r.left) / r.width;
      const y = (e.clientY - r.top) / r.height;
      const max = el.classList.contains("screen") ? 4 : 6;
      el.classList.add("tilting");
      el.style.setProperty("--ry", `${((x - 0.5) * max * 2).toFixed(2)}deg`);
      el.style.setProperty("--rx", `${((0.5 - y) * max * 2).toFixed(2)}deg`);
      el.style.setProperty("--mx", `${(x * 100).toFixed(1)}%`);
      el.style.setProperty("--my", `${(y * 100).toFixed(1)}%`);
    });
    el.addEventListener("pointerleave", () => {
      el.classList.remove("tilting");
      el.style.setProperty("--rx", "0deg");
      el.style.setProperty("--ry", "0deg");
    });
  }
}

/* ---------------- header, reveal ---------------- */

const header = $(".top");
const onScroll = () => header.classList.toggle("scrolled", scrollY > 24);
addEventListener("scroll", onScroll, { passive: true });
onScroll();

// Anything that has reached the viewport (or was scrolled past quickly) shows.
const hidden = $$(".stat, .step, .card, .road-item, .plan, .faq details, .split > *, .models, .tour .screen, .tabs, .final > :not(.final-scene)");
hidden.forEach((el, i) => {
  el.classList.add("reveal");
  el.style.transitionDelay = `${(i % 3) * 70}ms`;
});
function revealCheck() {
  // At the very bottom everything left is on screen anyway.
  const bottom = scrollY + innerHeight >= document.documentElement.scrollHeight - 4;
  const edge = bottom ? Infinity : innerHeight * 0.92;
  for (let i = hidden.length - 1; i >= 0; i--) {
    if (hidden[i].getBoundingClientRect().top < edge) {
      hidden[i].classList.add("in");
      hidden.splice(i, 1);
    }
  }
}
addEventListener("scroll", () => hidden.length && revealCheck(), { passive: true });
addEventListener("resize", revealCheck);
revealCheck();

/* ---------------- 3D objects follow the pointer ---------------- */

const floats = $$(".hero-stage .float");
const stage = $("#stage");
const mainArt = $(".f-main");
const ready = () => requestAnimationFrame(() => stage.classList.add("ready"));
if (mainArt.complete) ready();
else mainArt.addEventListener("load", ready, { once: true });
setTimeout(ready, 1500);
if (!reduced && matchMedia("(pointer: fine)").matches) {
  addEventListener(
    "pointermove",
    (e) => {
      const x = e.clientX / innerWidth - 0.5;
      const y = e.clientY / innerHeight - 0.5;
      for (const f of floats) {
        const d = Number(f.dataset.depth || 20);
        f.style.setProperty("--px", `${(-x * d).toFixed(1)}px`);
        f.style.setProperty("--py", `${(-y * d).toFixed(1)}px`);
      }
    },
    { passive: true },
  );
}

/* ---------------- the island ---------------- */

const SCENES = {
  ru: [
    {
      heard: "Джарвис, громкость 30 и открой последний проект",
      steps: [["♪", "Громкость 30%", "было 64%"], ["⚡", "VS Code: jarvis-island", "окно открыто"]],
      reply: "Готово. Громкость тридцать, проект открыт.",
    },
    {
      heard: "Поставь фокус на 25 минут",
      steps: [["◎", "Фокус 25 минут", "до 14:55"]],
      reply: "Фокус пошёл. Позову в 14:55.",
      meta: "24:59",
    },
    {
      heard: "Разложи браузер и код пополам",
      steps: [["▦", "Окна 50/50", "Chrome · VS Code"]],
      reply: "Разложил: браузер слева, код справа.",
    },
    {
      heard: "Спланируй мне завтра",
      steps: [["◷", "Календарь на завтра", "3 свободных окна"], ["✦", "Блоки", "работа · английский · зал"]],
      reply: "Глубокая работа в 10, английский в 15, зал в 19. Принять?",
    },
    {
      heard: "Что сейчас играет?",
      steps: [],
      reply: "The Weeknd, «Blinding Lights». Сделать погромче?",
    },
  ],
  en: [
    {
      heard: "Jarvis, volume 30 and open my last project",
      steps: [["♪", "Volume 30%", "was 64%"], ["⚡", "VS Code: jarvis-island", "window open"]],
      reply: "Done. Volume at thirty, project open.",
    },
    {
      heard: "Focus for 25 minutes",
      steps: [["◎", "Focus 25 minutes", "until 2:55 PM"]],
      reply: "Focus is on. I’ll call you at 2:55.",
      meta: "24:59",
    },
    {
      heard: "Put the browser and code side by side",
      steps: [["▦", "Windows 50/50", "Chrome · VS Code"]],
      reply: "Done: browser on the left, code on the right.",
    },
    {
      heard: "Plan my tomorrow",
      steps: [["◷", "Tomorrow’s calendar", "3 free slots"], ["✦", "Blocks", "deep work · English · gym"]],
      reply: "Deep work at 10, English at 3, gym at 7. Accept?",
    },
    {
      heard: "What’s playing right now?",
      steps: [],
      reply: "The Weeknd, “Blinding Lights”. Turn it up?",
    },
  ],
};
const LABELS = {
  ru: { listen: "Слушаю", think: "Делаю", speak: "Говорю", done: "Готово" },
  en: { listen: "Listening", think: "Working", speak: "Speaking", done: "Done" },
};

const island = $("#island");
const islandContent = $("#islandContent");
const body = $("#islandBody");
const label = $("#islandLabel");
const meta = $("#islandMeta");
const COMPACT = 174;

function size(w) {
  const width = Math.min(w, innerWidth - 36);
  islandContent.style.setProperty("--w", `${width}px`);
  island.style.width = `${width}px`;
  island.style.height = `${w === COMPACT ? 36 : islandContent.offsetHeight}px`;
  island.classList.toggle("big", w !== COMPACT);
}

function state(s) {
  island.dataset.state = s;
}

const bars = () => `<span class="i-bars">${"<i></i>".repeat(5)}</span>`;

// Fake sound level: moves the eyes while listening or speaking.
let levelTimer = 0;
function level(on) {
  clearInterval(levelTimer);
  island.style.setProperty("--lvl", 0);
  if (!on || reduced) return;
  levelTimer = setInterval(() => {
    const v = Math.random() ** 1.5;
    island.style.setProperty("--lvl", v.toFixed(2));
    $$(".i-bars i", island).forEach((b) => b.style.setProperty("--h", Math.random().toFixed(2)));
  }, 110);
}

// Blinks while idle, and the eyes follow the pointer.
let blinkTimer = 0;
function blinkLoop() {
  clearTimeout(blinkTimer);
  blinkTimer = setTimeout(() => {
    if (island.dataset.state === "idle") {
      island.classList.add("blink");
      setTimeout(() => island.classList.remove("blink"), 120);
    }
    blinkLoop();
  }, 3500 + Math.random() * 5000);
}
addEventListener(
  "pointermove",
  (e) => {
    const r = island.getBoundingClientRect();
    const dx = (e.clientX - (r.left + r.width / 2)) / innerWidth;
    island.style.setProperty("--look", `${Math.max(-3, Math.min(3, dx * 8)).toFixed(1)}px`);
  },
  { passive: true },
);

const demo = {
  run: 0,
  scene: 0,
  visible: true,
  async play() {
    const run = ++this.run;
    const alive = () => run === this.run;
    const wait = async (ms) => {
      await sleep(ms);
      if (!alive()) throw new Error("stop");
    };
    try {
      for (;;) {
        await this.idle(2200);
        await wait(2200);
        if (!this.visible) {
          await new Promise((r) => (this.resume = r));
          if (!alive()) return;
          continue;
        }
        const sc = SCENES[lang][this.scene++ % SCENES[lang].length];
        const L = LABELS[lang];

        // listening
        state("listen");
        label.textContent = L.listen;
        meta.innerHTML = bars();
        body.innerHTML = `<p class="i-heard"><span></span><i class="caret"></i></p>`;
        size(440);
        level(true);
        await wait(500);
        const out = $(".i-heard span", body);
        for (const ch of sc.heard) {
          out.textContent += ch;
          size(440);
          await wait(reduced ? 0 : 34);
        }
        $(".caret", body)?.remove();
        await wait(450);

        // working
        level(false);
        if (sc.steps.length) {
          state("think");
          label.textContent = L.think;
          meta.textContent = sc.meta || "";
          const list = document.createElement("ul");
          list.className = "i-steps";
          body.append(list);
          for (const [ic, t, sub] of sc.steps) {
            const li = document.createElement("li");
            li.innerHTML = `<span class="ic">${ic}</span><span>${t}<span class="sub">${sub}</span></span><span class="st"></span>`;
            list.append(li);
            size(460);
            await wait(750);
            li.classList.add("done");
            await wait(200);
          }
        } else {
          state("think");
          label.textContent = L.think;
          await wait(600);
        }

        // speaking
        state("speak");
        label.textContent = L.speak;
        meta.innerHTML = bars();
        const reply = document.createElement("p");
        reply.className = "i-reply";
        reply.textContent = sc.reply;
        body.append(reply);
        size(460);
        level(true);
        await wait(2600);

        // done
        level(false);
        state("happy");
        label.textContent = L.done;
        meta.textContent = "";
        await wait(1500);
      }
    } catch {
      /* restarted */
    }
  },
  async idle() {
    level(false);
    state("idle");
    label.textContent = "";
    meta.textContent = "";
    body.innerHTML = "";
    size(COMPACT);
  },
  restart() {
    this.run++;
    this.resume?.();
    this.idle();
    this.play();
  },
};

new IntersectionObserver(([e]) => {
  demo.visible = e.isIntersecting;
  if (demo.visible) demo.resume?.();
  else if (island.dataset.state !== "idle") demo.restart();
}).observe($("#hero"));

island.addEventListener("click", () => {
  scrollTo({ top: 0, behavior: reduced ? "auto" : "smooth" });
  demo.restart();
});

applyLang();
blinkLoop();
demo.play();
