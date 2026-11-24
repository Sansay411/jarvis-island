import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { isTauri } from "../lib/ipc";
import type { Emote } from "../store/emote";
import type {
  ActiveGameSnapshot,
  AllGameStats,
  BoardGameId,
  CommentRequest,
  CommentStreamEvent,
  Difficulty,
  PlayerMood,
  RemoteGameCommand,
  RemoteGameReply,
} from "./types";

const DEFAULT_STATS: AllGameStats = {
  chess: { wins: 0, losses: 0, draws: 0, streak: 0 },
  checkers: { wins: 0, losses: 0, draws: 0, streak: 0 },
  battleship: { wins: 0, losses: 0, draws: 0, streak: 0 },
};

let qaReqSeq = 100;
const qaListeners = new Set<(ev: CommentStreamEvent) => void>();

/** Contextual non-repeating browser preview simulation when running outside Tauri (`/qa/games.html`). */
function simulateQaComment(req: CommentRequest): number {
  const reqId = ++qaReqSeq;
  let emote: Emote = "wink";
  let moodSwitch: PlayerMood | null = null;
  let text = "";

  const say = (req.playerSay ?? "").trim().toLowerCase();
  if (say) {
    if (say.includes("делов") || say.includes("серьез") || say.includes("серьёз")) {
      moodSwitch = "business";
      emote = "determined";
      text = "Принято. Убираю шутки в сторону — переходим на строгий клубный расчёт.";
    } else if (say.includes("шут") || say.includes("весел")) {
      moodSwitch = "joker";
      emote = "laugh";
      text = "Хех, договорились! Тогда предупреждаю: мои кони сегодня с стендап-подготовкой.";
    } else {
      emote = req.mood === "business" ? "determined" : "laugh";
      text =
        req.mood === "business"
          ? `Интересная мысль. Судя по доске (${req.scoreSummary.toLowerCase()}), главная борьба ещё впереди.`
          : `Ахах, звучит как тонкий психологический приём! Но меня из чёлки монитора так просто не собьёшь — ${req.scoreSummary.toLowerCase()}.`;
    }
  } else if (req.trigger === "start") {
    if (req.mood === "business") {
      emote = "determined";
      text =
        req.game === "chess"
          ? "Фигуры расставлены, белые ваши. Жду первого хода в центре."
          : req.game === "checkers"
            ? "Доска готова. В русских шашках темп решает всё — начинайте."
            : "Квадраты сверены, корабли на позициях. Ваш первый залп.";
    } else if (req.mood === "joker") {
      emote = "laugh";
      text =
        req.game === "chess"
          ? "Ну что, гроссмейстер, у меня нет рук, чтобы уронить короля, так что придётся выигрывать честно!"
          : req.game === "checkers"
            ? "Погнали в чапаевские… ладно-ладно, в настоящие русские шашки! В дамки без очереди не пускаю."
            : "Адмирал Джарвис поднял перископ! Обещаю в твои клетки не подглядывать — у меня радар честный.";
    } else {
      emote = "wink";
      text =
        req.game === "chess"
          ? "Партия началась. Посмотрим, чей план окажется острее."
          : req.game === "checkers"
            ? "Белые начинают — проверим, кто быстрее прорвётся в дамки."
            : "Флот вышел в море. Делай первый выстрел, проверим твою интуицию!";
    }
  } else if (req.tacticalNotes.length > 0) {
    const note = req.tacticalNotes[0]!;
    if (note.includes("МАТ") || note.includes("победил") || note.includes("выиграл")) {
      emote = note.includes("Игрок") ? "star" : "wink";
      text = note.includes("Игрок")
        ? req.mood === "business"
          ? `${note} Точная реализация перевеса, снимаю шляпу.`
          : `${note} Ого, красиво приложил! Требую немедленный реванш.`
        : req.mood === "business"
          ? `${note} Благодарю за плотную партию — сыграем ещё одну?`
          : `${note} Хех, партия за пентхаусом над панелью задач! Готов отыграться?`;
    } else {
      emote = req.mood === "business" ? "determined" : note.includes("Игрок") ? "surprised" : "laugh";
      const replyPart = req.jarvisMove ? ` Мой ответ — ${req.jarvisMove}.` : "";
      text =
        req.mood === "business"
          ? `${note}.${replyPart} Позиция требует точного счёта.`
          : `${note}!${replyPart} Тут уже запахло настоящей тактикой.`;
    }
  } else {
    emote = req.mood === "business" ? "determined" : "squint";
    text =
      req.mood === "business"
        ? `На ${req.playerMove ?? "ваш манёвр"} отвечаю ${req.jarvisMove ?? "по центру"}. ${req.scoreSummary}.`
        : `Хм, ${req.playerMove ?? "хитро"}… Ладно, держи ${req.jarvisMove ?? "ответку"} — посмотрим, куда ты клонишь!`;
  }

  const words = text.split(" ");
  let idx = 0;
  window.setTimeout(() => {
    qaListeners.forEach((fn) => fn({ reqId, emote, moodSwitch }));
    const timer = window.setInterval(() => {
      if (idx >= words.length) {
        window.clearInterval(timer);
        qaListeners.forEach((fn) => fn({ reqId, done: true, fullText: text, emote, moodSwitch }));
        return;
      }
      const chunk = (idx === 0 ? "" : " ") + words[idx]!;
      idx++;
      qaListeners.forEach((fn) => fn({ reqId, delta: chunk }));
    }, 45);
  }, 140);

  return reqId;
}

export const gamesIpc = {
  open(game?: BoardGameId | null, difficulty?: Difficulty, mood?: PlayerMood): Promise<void> {
    if (!isTauri()) {
      window.open(`/qa/games.html${game ? `?game=${game}` : ""}`, "_blank");
      return Promise.resolve();
    }
    return invoke("games_open", {
      game: game ?? null,
      difficulty: difficulty ?? null,
      mood: mood ?? null,
    });
  },

  ready(): Promise<RemoteGameCommand | null> {
    if (!isTauri()) return Promise.resolve(null);
    return invoke<RemoteGameCommand | null>("games_ready");
  },

  stats(): Promise<AllGameStats> {
    if (!isTauri()) {
      try {
        const raw = localStorage.getItem("jarvis.qa.games.stats");
        return Promise.resolve(raw ? (JSON.parse(raw) as AllGameStats) : DEFAULT_STATS);
      } catch {
        return Promise.resolve(DEFAULT_STATS);
      }
    }
    return invoke<AllGameStats>("games_stats");
  },

  recordFinish(game: BoardGameId, winner: "player" | "jarvis" | "draw"): Promise<AllGameStats> {
    if (!isTauri()) {
      return this.stats().then((all) => {
        const next: AllGameStats = JSON.parse(JSON.stringify(all));
        const e = next[game];
        if (winner === "player") {
          e.wins += 1;
          e.streak = e.streak >= 0 ? e.streak + 1 : 1;
        } else if (winner === "jarvis") {
          e.losses += 1;
          e.streak = e.streak <= 0 ? e.streak - 1 : -1;
        } else {
          e.draws += 1;
          e.streak = 0;
        }
        try {
          localStorage.setItem("jarvis.qa.games.stats", JSON.stringify(next));
        } catch {
          /* ignore */
        }
        return next;
      });
    }
    return invoke<AllGameStats>("games_record_finish", { game, winner });
  },

  syncSnapshot(snapshot: ActiveGameSnapshot): void {
    if (!isTauri()) return;
    void invoke("games_sync_snapshot", { snapshot }).catch(() => {});
  },

  replyCmd(reply: RemoteGameReply): void {
    if (!isTauri()) return;
    void invoke("games_reply_cmd", { reply }).catch(() => {});
  },

  comment(req: CommentRequest): Promise<number> {
    if (!isTauri()) {
      return Promise.resolve(simulateQaComment(req));
    }
    return invoke<number>("games_comment", { req });
  },

  onComment(cb: (ev: CommentStreamEvent) => void): Promise<() => void> {
    if (!isTauri()) {
      qaListeners.add(cb);
      return Promise.resolve(() => {
        qaListeners.delete(cb);
      });
    }
    return listen<CommentStreamEvent>("games:comment", (e) => cb(e.payload));
  },

  onRemoteCmd(cb: (cmd: RemoteGameCommand) => void): Promise<() => void> {
    if (!isTauri()) return Promise.resolve(() => {});
    return listen<RemoteGameCommand>("games:remote-cmd", (e) => cb(e.payload));
  },

  onStats(cb: (stats: AllGameStats) => void): Promise<() => void> {
    if (!isTauri()) return Promise.resolve(() => {});
    return listen<AllGameStats>("games:stats", (e) => cb(e.payload));
  },
};
