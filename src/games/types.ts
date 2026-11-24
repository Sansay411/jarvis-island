import type { Emote } from "../store/emote";

export type BoardGameId = "chess" | "checkers" | "battleship";
export type Difficulty = "easy" | "medium" | "hard";
export type PlayerMood = "auto" | "joker" | "business" | "rival";

export interface GameStatsEntry {
  wins: number;
  losses: number;
  draws: number;
  streak: number;
}

export type AllGameStats = Record<BoardGameId, GameStatsEntry>;

export interface ActiveGameSnapshot {
  open: boolean;
  game: BoardGameId | null;
  phase: "lobby" | "setup" | "playing" | "finished";
  difficulty: Difficulty;
  mood: PlayerMood;
  voiceEnabled: boolean;
  turn: "player" | "jarvis" | "none";
  moveCount: number;
  scoreSummary: string;
  boardSummary: string;
  lastMoves: string[];
  winner: "player" | "jarvis" | "draw" | null;
  statusLine: string;
}

export interface CommentRequest {
  game: BoardGameId;
  difficulty: Difficulty;
  mood: PlayerMood;
  trigger:
    | "start"
    | "move"
    | "capture"
    | "critical"
    | "finish"
    | "chat"
    | "mood_change"
    | "hint";
  moveCount: number;
  playerMove?: string | null;
  jarvisMove?: string | null;
  tacticalNotes: string[];
  scoreSummary: string;
  boardSummary: string;
  recentJarvisLines: string[];
  playerSay?: string | null;
  speak: boolean;
}

export interface CommentStreamEvent {
  reqId: number;
  delta?: string;
  done?: boolean;
  fullText?: string;
  emote?: Emote | null;
  moodSwitch?: PlayerMood | null;
}

export interface RemoteGameCommand {
  requestId: string;
  action: "open" | "new_game" | "move" | "hint" | "set_mood" | "resign";
  game?: BoardGameId | null;
  difficulty?: Difficulty | null;
  mood?: PlayerMood | null;
  moveStr?: string | null;
}

export interface RemoteGameReply {
  requestId: string;
  ok: boolean;
  summary: string;
  data?: Record<string, unknown>;
}

export const GAME_META: Record<
  BoardGameId,
  {
    title: string;
    subtitle: string;
    description: string;
    badge: string;
    rulesHint: string;
  }
> = {
  chess: {
    title: "Шахматы",
    subtitle: "Классика FIDE · дебюты и тактика",
    description: "Рокировки, взятие на проходе, дебютный репертуар и живой разбор комбинаций за доской.",
    badge: "8×8 · FIDE",
    rulesHint: "Ходите кликом, перетаскиванием или голосом («конём на f3», «е2-е4»).",
  },
  checkers: {
    title: "Русские шашки",
    subtitle: "Дамки через всё поле · бой назад",
    description: "Настоящие русские правила: обязательный бой вперёд и назад, серии прыжков и дальнобойные дамки.",
    badge: "8×8 · Русские",
    rulesHint: "Бить обязательно. Простая шашка бьёт и вперёд, и назад; дамка ходит на любое число полей.",
  },
  battleship: {
    title: "Морской бой",
    subtitle: "Флот 10 кораблей · честная охота",
    description: "Линкор, крейсера, эсминцы и катера. Джарвис не подглядывает в ваше поле, а вычисляет вероятности.",
    badge: "10×10 · Флот",
    rulesHint: "Попадание даёт повторный залп. Вокруг потопленного корабля вода открывается автоматически.",
  },
};

export const DIFFICULTY_LABELS: Record<Difficulty, { label: string; sub: string }> = {
  easy: { label: "Лёгкий", sub: "Тёплая разминка" },
  medium: { label: "Соперник", sub: "Крепкий клубный уровень" },
  hard: { label: "Мастер", sub: "Глубокий расчёт" },
};

export const MOOD_LABELS: Record<PlayerMood, { label: string; desc: string }> = {
  auto: {
    label: "Свой",
    desc: "Манера и уровень юмора из настроек Джарвиса",
  },
  joker: {
    label: "Шутник",
    desc: "Шутит, смеётся, травит байки и самоиронизирует",
  },
  rival: {
    label: "Азартный",
    desc: "Дружеские подколки, интрига и живой спортивный азарт",
  },
  business: {
    label: "Деловой",
    desc: "Собранный, точный и уважительный партнёр по клубу",
  },
};
