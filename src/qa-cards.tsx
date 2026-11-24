// Visual & interactive QA showcase for JARVIS Explanation Cards.
// Served at `/qa/cards.html?bg=light|dark&mode=overlay|compact`.

import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource-variable/onest";
import "@fontsource-variable/jetbrains-mono";
import "./styles/tokens.css";
import "./styles/base.css";
import { CardView } from "./overlay/cards/CardView";
import { normalizeCard } from "./overlay/cards/normalize";
import type { CardSpec } from "./overlay/cards/types";

interface DemoCard {
  id: string;
  tag: string;
  spec: CardSpec;
}

const DEMO_CARDS: DemoCard[] = [
  {
    id: "dns",
    tag: "flow (td) · steps · callout",
    spec: {
      title: "Как работает DNS",
      subtitle: "Путь запроса от доменного имени до IP-адреса за ~25 мс",
      blocks: [
        {
          kind: "flow",
          direction: "td",
          nodes: [
            { id: "browser", label: "Браузер", note: "example.com", tone: "accent" },
            { id: "resolver", label: "Резолвер", note: "провайдер / 1.1.1.1" },
            { id: "root", label: "Корневой DNS", note: "зона ." },
            { id: "tld", label: "TLD-сервер", note: "зона .com" },
            { id: "auth", label: "Авторитативный", note: "93.184.216.34", tone: "accent" },
          ],
          edges: [
            { from: "browser", to: "resolver", label: "запрос" },
            { from: "resolver", to: "root", label: "где .com?" },
            { from: "resolver", to: "tld", label: "где example?" },
            { from: "root", to: "tld" },
            { from: "tld", to: "auth", label: "NS-запись" },
          ],
        },
        {
          kind: "steps",
          items: [
            { title: "Проверка локального кэша", note: "Браузер и ОС ищут запись в памяти, прежде чем идти в сеть." },
            { title: "Рекурсивный поиск", note: "Резолвер обходит корневой сервер, TLD и авторитативный DNS." },
            { title: "Сохранение по TTL", note: "Ответ кэшируется на время жизни записи (обычно от 60 с до суток)." },
          ],
        },
        {
          kind: "callout",
          tone: "tip",
          title: "Как проверить у себя",
          text: "Выполните `nslookup example.com` в терминале — увидите адрес резолвера и полученный IP.",
        },
      ],
    },
  },
  {
    id: "phones",
    tag: "compare · table · kv",
    spec: {
      title: "iPhone 16 Pro vs Pixel 9 Pro",
      subtitle: "Короткое сравнение по камере, автономности и экосистеме",
      blocks: [
        {
          kind: "compare",
          columns: [
            {
              title: "iPhone 16 Pro",
              subtitle: "iOS 18 · A18 Pro",
              accent: true,
              items: [
                { text: "Лучшая запись видео (4K 120 fps Log)", kind: "pro" },
                { text: "Высокая производительность и стабильный FPS", kind: "pro" },
                { text: "Медленнее зарядка по кабелю", kind: "con" },
              ],
            },
            {
              title: "Pixel 9 Pro",
              subtitle: "Android 15 · Tensor G4",
              items: [
                { text: "Очень сильные фото в темноте и HDR", kind: "pro" },
                { text: "Гибкая система и функции ИИ на устройстве", kind: "pro" },
                { text: "Слабее в тяжёлых 3D-играх", kind: "con" },
              ],
            },
          ],
        },
        {
          kind: "table",
          headers: ["Параметр", "iPhone 16 Pro", "Pixel 9 Pro"],
          highlightCol: 1,
          rows: [
            ["Экран", "6.3″ 120 Гц", "6.3″ 120 Гц"],
            ["Зум-камера", "5× 12 МП", "5× 48 МП"],
            ["Вес", "199 г", "199 г"],
            ["Поддержка ОС", "6–7 лет", "7 лет"],
          ],
        },
        {
          kind: "kv",
          items: [
            { key: "Для видео и Mac", value: "iPhone 16 Pro" },
            { key: "Для фото и Android", value: "Pixel 9 Pro" },
          ],
        },
      ],
    },
  },
  {
    id: "revenue",
    tag: "bars · line · callout",
    spec: {
      title: "Рост выручки по кварталам",
      subtitle: "Сравнение выручки и чистой маржи за 2025 год",
      blocks: [
        {
          kind: "bars",
          title: "Квартальная выручка",
          unit: "млн ₽",
          items: [
            { label: "Q1 2025", value: 42.5 },
            { label: "Q2 2025", value: 58.0 },
            { label: "Q3 2025", value: 71.2 },
            { label: "Q4 2025", value: 96.4, highlight: true },
          ],
        },
        {
          kind: "line",
          title: "План и факт по месяцам",
          unit: "млн ₽",
          xLabels: ["Янв", "Мар", "Май", "Июл", "Сен", "Ноя"],
          series: [
            { name: "Факт", values: [13, 16, 21, 23, 26, 35] },
            { name: "План", values: [14, 16, 18, 20, 23, 28] },
          ],
        },
        {
          kind: "callout",
          tone: "idea",
          title: "Главный драйвер",
          text: "В **Q4** сработал запуск годовых подписок: доля предоплат выросла с *18%* до **34%**.",
        },
      ],
    },
  },
  {
    id: "python",
    tag: "timeline · definition · text",
    spec: {
      title: "История Python",
      subtitle: "От рождественского хобби-проекта до языка науки о данных",
      blocks: [
        {
          kind: "timeline",
          items: [
            { date: "1991", label: "Python 0.9.0", note: "Гвидо ван Россум публикует первый релиз с классами и исключениями." },
            { date: "2000", label: "Python 2.0", note: "Появляются генераторы списков и сборщик циклов." },
            { date: "2008", label: "Python 3.0", note: "Юникод для строк по умолчанию, чистка синтаксиса." },
            { date: "2024+", label: "Python 3.13", note: "Экспериментальный free-threaded режим без GIL и базовый JIT." },
          ],
        },
        {
          kind: "definition",
          term: "GIL (Global Interpreter Lock)",
          meaning: "Глобальная блокировка в CPython, позволяющая выполнять байт-код Python только одному потоку одновременно.",
          example: "Для CPU-задач используют multiprocessing или модули на Rust/C.",
        },
        {
          kind: "text",
          text: "Почему язык остался популярным:\n- **Читаемость**: код почти читается как *псевдокод*.\n- **Экосистема**: `NumPy`, `PyTorch`, `FastAPI` закрывают 90% задач.",
        },
      ],
    },
  },
  {
    id: "math-quiz",
    tag: "math · quiz · callout",
    spec: {
      title: "Квадратное уравнение и проверка",
      subtitle: "Разбор через дискриминант и быстрый тест для закрепления",
      blocks: [
        {
          kind: "math",
          lines: [
            // Written the way the model types it: prettyMath sets it.
            { expr: "2x^2 - 5x + 2 = 0", note: "исходное уравнение" },
            { expr: "D = (-5)^2 - 4*2*2 = 25 - 16 = 9", note: "дискриминант" },
            { expr: "sqrt(D) = 3", note: "два вещественных корня" },
            { expr: "x_1 = (5 + 3)/4 = 2", note: "первый корень" },
            { expr: "x_2 = (5 - 3)/4 = 0.5", note: "второй корень" },
          ],
          result: "x_1 = 2, x_2 = 0.5",
        },
        {
          kind: "quiz",
          question: "Сколько вещественных корней у уравнения x² + 4x + 5 = 0?",
          options: ["Два разных корня", "Один двойной корень", "Ни одного вещественного корня"],
          correct: 2,
          explanation: "Дискриминант D = 16 − 20 = −4 < 0, поэтому корни только комплексные.",
        },
        {
          kind: "callout",
          tone: "warning",
          text: "Не забывайте делить на **2a**, а не просто на 2 — при `a ≠ 1` это самая частая ошибка.",
        },
      ],
    },
  },
  {
    id: "http-cache",
    tag: "flow (lr + back-edge)",
    spec: {
      title: "HTTP-кэш с ETag",
      subtitle: "Горизонтальная схема с циклом повторной валидации",
      blocks: [
        {
          kind: "flow",
          direction: "lr",
          nodes: [
            { id: "c", label: "Клиент", note: "If-None-Match", tone: "accent" },
            { id: "s", label: "Сервер", note: "сверка ETag" },
            { id: "ok", label: "304 Not Modified", note: "из кэша", tone: "muted" },
          ],
          edges: [
            { from: "c", to: "s", label: "GET" },
            { from: "s", to: "ok", label: "совпал" },
            { from: "ok", to: "c", label: "повтор" },
          ],
        },
      ],
    },
  },
  {
    id: "sloppy",
    tag: "небрежный ответ модели → normalizeCard",
    spec: normalizeCard({
      heading: "Как устроен TLS",
      blocks: JSON.stringify([
        { type: "diagram", edges: ["Клиент -> Сервер: ClientHello", "Сервер -> Клиент: сертификат", "Клиент -> Ключи: обмен"] },
        { type: "facts", items: [["Порт", 443], { label: "Версия", value: "1.3" }] },
        { type: "chart", data: [{ name: "TLS 1.2", value: "2 RTT" }, { name: "TLS 1.3", value: "1 RTT", highlight: "true" }], unit: "RTT" },
        { question: "Сколько обменов нужно TLS 1.3?", options: ["Один", "Два", "Три"], answer: "Один" },
      ]),
    })!,
  },
];

function initialParam(name: string, fallback: string): string {
  if (typeof window === "undefined") return fallback;
  return new URLSearchParams(window.location.search).get(name) ?? fallback;
}

function QaCardsApp() {
  const [bg, setBg] = useState<"dark" | "light">(() => (initialParam("bg", "dark") === "light" ? "light" : "dark"));
  const [compact, setCompact] = useState<boolean>(() => initialParam("mode", "overlay") === "compact");
  const [closed, setClosed] = useState<Record<string, boolean>>({});

  useEffect(() => {
    document.documentElement.style.overflow = "auto";
    document.documentElement.style.height = "auto";
    document.body.style.overflow = "auto";
    document.body.style.height = "auto";
    const root = document.getElementById("root");
    if (root) {
      root.style.overflow = "visible";
      root.style.height = "auto";
    }
  }, []);

  const isLight = bg === "light";
  const closedCount = Object.values(closed).filter(Boolean).length;

  return (
    <div
      style={{
        minHeight: "100vh",
        padding: "28px 28px 92px",
        background: isLight
          ? "radial-gradient(circle at 20% 0%, #ffffff 0%, #eef0f4 55%, #e2e6ee 100%)"
          : "radial-gradient(90% 60% at 50% 0%, #1d212b 0%, #0d0f14 68%, #08090c 100%)",
        color: isLight ? "#1b1d24" : "#f2f2f0",
        transition: "background 220ms ease",
      }}
    >
      {/* Realistic backdrop text to verify glass contrast over light/dark reading */}
      <header style={{ maxWidth: 1360, margin: "0 auto 22px" }}>
        <div style={{ display: "flex", alignItems: "baseline", justifyContent: "space-between", flexWrap: "wrap", gap: 12 }}>
          <div>
            <h1 style={{ margin: 0, fontSize: 18, fontWeight: 640, letterSpacing: "-0.02em" }}>
              JARVIS · Пояснительные карточки (QA)
            </h1>
            <p style={{ margin: "4px 0 0", fontSize: 12.5, opacity: 0.68 }}>
              Все 13 типов блоков · фон: {isLight ? "светлая страница" : "тёмный экран"} · режим: {compact ? "встроенный в чат" : "поверх окна"}
            </p>
          </div>
          {closedCount > 0 && (
            <button
              type="button"
              onClick={() => setClosed({})}
              style={{
                height: 28,
                padding: "0 12px",
                borderRadius: 999,
                fontSize: 12,
                fontWeight: 560,
                background: isLight ? "#16181f" : "#ecebe6",
                color: isLight ? "#f2f2f0" : "#08080a",
              }}
            >
              Вернуть скрытые ({closedCount})
            </button>
          )}
        </div>
      </header>

      <main
        style={{
          maxWidth: 1360,
          margin: "0 auto",
          display: "grid",
          gridTemplateColumns: "repeat(auto-fill, minmax(360px, 420px))",
          gap: 24,
          alignItems: "start",
          justifyContent: "center",
        }}
      >
        {DEMO_CARDS.filter((c) => !closed[c.id]).map((card) => (
          <div key={card.id} style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            <span
              style={{
                fontFamily: "var(--font-mono)",
                fontSize: 10.5,
                opacity: 0.58,
                paddingLeft: 4,
              }}
            >
              {card.tag}
            </span>
            <CardView
              spec={card.spec}
              compact={compact}
              onClose={compact ? undefined : () => setClosed((prev) => ({ ...prev, [card.id]: true }))}
            />
          </div>
        ))}
      </main>

      <nav
        aria-label="Настройки QA-витрины"
        style={{
          position: "fixed",
          bottom: 16,
          left: "50%",
          transform: "translateX(-50%)",
          zIndex: 999,
          display: "flex",
          alignItems: "center",
          gap: 6,
          padding: "6px 8px",
          borderRadius: 999,
          background: "rgba(12, 12, 15, 0.92)",
          border: "1px solid rgba(255, 255, 255, 0.15)",
          boxShadow: "0 14px 36px rgba(0, 0, 0, 0.45)",
          backdropFilter: "blur(12px)",
        }}
      >
        {(
          [
            ["dark", "Тёмный фон"],
            ["light", "Светлый фон"],
          ] as const
        ).map(([val, label]) => (
          <button
            key={val}
            type="button"
            onClick={() => setBg(val)}
            style={{
              height: 26,
              padding: "0 12px",
              borderRadius: 999,
              fontSize: 11.5,
              fontWeight: 560,
              background: bg === val ? "#f2f2f0" : "transparent",
              color: bg === val ? "#08080a" : "#a4a4aa",
            }}
          >
            {label}
          </button>
        ))}

        <span style={{ width: 1, height: 14, background: "rgba(255,255,255,0.14)", margin: "0 2px" }} />

        <button
          type="button"
          onClick={() => setCompact((v) => !v)}
          style={{
            height: 26,
            padding: "0 12px",
            borderRadius: 999,
            fontSize: 11.5,
            fontWeight: 560,
            background: compact ? "rgba(255, 178, 63, 0.18)" : "rgba(255, 255, 255, 0.06)",
            color: compact ? "#ffd88f" : "#a4a4aa",
            border: compact ? "1px solid rgba(255, 178, 63, 0.45)" : "1px solid transparent",
          }}
        >
          {compact ? "Режим: в чате (compact)" : "Режим: поверх экрана"}
        </button>
      </nav>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(<QaCardsApp />);
