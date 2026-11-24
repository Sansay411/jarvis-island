import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Eyes } from "../island/Eyes";
import { skinCssVars } from "../island/skins/types";
import { isTauri } from "../lib/ipc";
import { ART } from "../mentor/art";
import { TitleBar } from "../settings/TitleBar";
import type { Emote } from "../store/emote";
import { startPrefsFeed, usePrefs } from "../store/prefs";
import { startTtsFeed, useTts } from "../store/tts";
import { outputLevel, startVoiceFeed, toggleVoice, useVoice } from "../store/voice";
import { Icon } from "../ui/Icon";
import {
  battleshipFleetSummary,
  chooseBattleshipShot,
  createBattleshipState,
  fireJarvisShot,
  firePlayerShot,
  hintBattleshipShot,
  parseSpokenBattleshipShot,
  randomFleet,
  startBattleshipMatch,
  type BattleshipState,
} from "./battleship";
import { BattleshipBoard } from "./BattleshipBoard";
import {
  applyCheckersMove,
  checkersBoardText,
  checkersScoreSummary,
  chooseCheckersMove,
  createCheckersState,
  describeCheckersExchange,
  hintCheckersMove,
  parseSpokenCheckersMove,
  type CheckersMove,
  type CheckersState,
} from "./checkers";
import { CheckersBoard } from "./CheckersBoard";
import {
  applyChessMove,
  chooseChessMove,
  createChessState,
  describeChessExchange,
  hintChessMove,
  materialBalanceSummary,
  parseSpokenChessMove,
  toFen,
  type ChessMove,
  type ChessState,
} from "./chess";
import { ChessBoard } from "./ChessBoard";
import { gamesIpc } from "./ipc";
import { setSfxMuted, sfx } from "./sfx";
import {
  DIFFICULTY_LABELS,
  GAME_META,
  MOOD_LABELS,
  type ActiveGameSnapshot,
  type AllGameStats,
  type BoardGameId,
  type CommentRequest,
  type Difficulty,
  type PlayerMood,
  type RemoteGameCommand,
} from "./types";
import "./games.css";

interface ChatLine {
  id: number;
  who: "jarvis" | "player" | "system";
  text: string;
}

const BANTER_CHIPS: Array<{ label: string; say: string }> = [
  { label: "Хороший ход!", say: "Красивый ход, признаю!" },
  { label: "Почему так сходил?", say: "Объясни коротко идею своего последнего хода." },
  { label: "Пошути", say: "Разряди обстановку, пошути по позиции!" },
  { label: "Играй серьёзно", say: "Давай теперь строго по-деловому, без шуток." },
  { label: "Сдавайся!", say: "Признайся, твоя позиция уже трещит по швам!" },
];

const GAME_ART: Record<BoardGameId, string> = {
  chess: ART.chess,
  checkers: ART.checkers,
  battleship: ART.battleship,
};

function readInitialQuery(): { game: BoardGameId | null; diff: Difficulty; mood: PlayerMood } {
  if (typeof window === "undefined") return { game: null, diff: "medium", mood: "auto" };
  const q = new URLSearchParams(window.location.search);
  const g = q.get("game");
  const d = q.get("difficulty");
  const m = q.get("mood");
  return {
    game: g === "chess" || g === "checkers" || g === "battleship" ? g : null,
    diff: d === "easy" || d === "medium" || d === "hard" ? d : "medium",
    mood: m === "auto" || m === "joker" || m === "business" || m === "rival" ? m : "auto",
  };
}

export default function GamesApp() {
  const init = useMemo(readInitialQuery, []);
  const skin = usePrefs((s) => s.skin);
  const ttsSpeaking = useTts((s) => s.speaking);
  const stopTts = useTts((s) => s.stop);
  const voiceState = useVoice((s) => s.state);

  const [activeGame, setActiveGame] = useState<BoardGameId | null>(init.game);
  const [difficulty, setDifficulty] = useState<Difficulty>(init.diff);
  const [mood, setMood] = useState<PlayerMood>(init.mood);
  const [speakComments, setSpeakComments] = useState<boolean>(() => {
    try {
      return localStorage.getItem("jarvis.games.speak") !== "0";
    } catch {
      return true;
    }
  });
  const [sfxOn, setSfxOn] = useState<boolean>(() => {
    try {
      const on = localStorage.getItem("jarvis.games.sfx") !== "0";
      setSfxMuted(!on);
      return on;
    } catch {
      return true;
    }
  });

  const [stats, setStats] = useState<AllGameStats>({
    chess: { wins: 0, losses: 0, draws: 0, streak: 0 },
    checkers: { wins: 0, losses: 0, draws: 0, streak: 0 },
    battleship: { wins: 0, losses: 0, draws: 0, streak: 0 },
  });

  // Board states + undo stacks
  const [chess, setChess] = useState<ChessState>(createChessState);
  const [chessUndo, setChessUndo] = useState<ChessState[]>([]);
  const [chessHint, setChessHint] = useState<ChessMove | null>(null);

  const [checkers, setCheckers] = useState<CheckersState>(createCheckersState);
  const [checkersUndo, setCheckersUndo] = useState<CheckersState[]>([]);
  const [checkersHint, setCheckersHint] = useState<CheckersMove | null>(null);

  const [battleship, setBattleship] = useState<BattleshipState>(createBattleshipState);
  const [bsHint, setBsHint] = useState<number | null>(null);
  const [bsMissStreak, setBsMissStreak] = useState(0);

  const [flipped, setFlipped] = useState(false);
  const [jarvisThinking, setJarvisThinking] = useState(false);

  // Jarvis's live face + streaming speech bubble + table log
  const [faceEmote, setFaceEmote] = useState<Emote | null>(null);
  const [faceEmoteSeq, setFaceEmoteSeq] = useState(0);
  const [liveSpeech, setLiveSpeech] = useState<string>(
    "Выбирай партию — шахматы, русские шашки или морской бой. Могу играть с шутками, с азартом или строго по-деловому.",
  );
  const [streamingSpeech, setStreamingSpeech] = useState(false);
  const [chatLog, setChatLog] = useState<ChatLine[]>([]);
  const [chatInput, setChatInput] = useState("");

  const recentJarvisLines = useRef<string[]>([]);
  const activeReqId = useRef<number>(0);
  const emoteTimer = useRef<number | undefined>(undefined);
  const chatSeq = useRef(1);
  const logEndRef = useRef<HTMLDivElement | null>(null);

  const triggerEmote = useCallback((em: Emote) => {
    window.clearTimeout(emoteTimer.current);
    setFaceEmote(em);
    setFaceEmoteSeq((s) => s + 1);
    emoteTimer.current = window.setTimeout(() => setFaceEmote(null), 1850);
  }, []);

  const pushLog = useCallback((who: ChatLine["who"], text: string) => {
    const clean = text.trim();
    if (!clean) return;
    setChatLog((prev) => [...prev.slice(-36), { id: chatSeq.current++, who, text: clean }]);
  }, []);

  useEffect(() => {
    logEndRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [chatLog, liveSpeech]);

  // Feeds & stats
  useEffect(() => {
    if (isTauri()) {
      void startPrefsFeed();
      void startTtsFeed();
      void startVoiceFeed();
    }
    void gamesIpc.stats().then(setStats);
    const unStats = gamesIpc.onStats(setStats);
    const unComment = gamesIpc.onComment((ev) => {
      if (ev.reqId < activeReqId.current) return;
      activeReqId.current = ev.reqId;
      if (ev.emote) triggerEmote(ev.emote);
      if (ev.moodSwitch) setMood(ev.moodSwitch);
      if (ev.delta) {
        setStreamingSpeech(true);
        setLiveSpeech((prev) => prev + ev.delta!);
      }
      if (ev.done) {
        setStreamingSpeech(false);
        if (ev.fullText) {
          setLiveSpeech(ev.fullText);
          recentJarvisLines.current = [...recentJarvisLines.current.slice(-8), ev.fullText];
          pushLog("jarvis", ev.fullText);
        }
      }
    });
    return () => {
      void unStats.then((f) => f());
      void unComment.then((f) => f());
    };
  }, [pushLog, triggerEmote]);

  // Build current snapshot for Rust context & status tool
  const buildSnapshot = useCallback((): ActiveGameSnapshot => {
    if (!activeGame) {
      return {
        open: true,
        game: null,
        phase: "lobby",
        difficulty,
        mood,
        voiceEnabled: speakComments,
        turn: "none",
        moveCount: 0,
        scoreSummary: "В лобби",
        boardSummary: "Игра ещё не выбрана",
        lastMoves: [],
        winner: null,
        statusLine: "Выбор игры в клубе Джарвиса",
      };
    }
    if (activeGame === "chess") {
      const winner =
        chess.winner === "w" ? "player" : chess.winner === "b" ? "jarvis" : chess.winner === "draw" ? "draw" : null;
      return {
        open: true,
        game: "chess",
        phase: chess.status === "playing" ? "playing" : "finished",
        difficulty,
        mood,
        voiceEnabled: speakComments,
        turn: chess.status !== "playing" ? "none" : chess.turn === "w" ? "player" : "jarvis",
        moveCount: chess.fullmove,
        scoreSummary: materialBalanceSummary(chess),
        boardSummary: `FEN: ${toFen(chess)}${chess.openingName ? ` (${chess.openingName})` : ""}`,
        lastMoves: chess.history.slice(-6).map((h) => h.ru),
        winner,
        statusLine:
          chess.status === "checkmate"
            ? winner === "player"
              ? "Мат! Победа игрока"
              : "Мат! Победа Джарвиса"
            : chess.status !== "playing"
              ? "Ничья"
              : chess.inCheck
                ? "Шах на доске"
                : chess.turn === "w"
                  ? "Ход белых (игрок)"
                  : "Ход чёрных (Джарвис)",
      };
    }
    if (activeGame === "checkers") {
      const sc = checkersScoreSummary(checkers);
      const winner =
        checkers.winner === "w"
          ? "player"
          : checkers.winner === "b"
            ? "jarvis"
            : checkers.winner === "draw"
              ? "draw"
              : null;
      return {
        open: true,
        game: "checkers",
        phase: checkers.status === "playing" ? "playing" : "finished",
        difficulty,
        mood,
        voiceEnabled: speakComments,
        turn: checkers.status !== "playing" ? "none" : checkers.turn === "w" ? "player" : "jarvis",
        moveCount: Math.ceil(checkers.history.length / 2) + 1,
        scoreSummary: sc.text,
        boardSummary: checkersBoardText(checkers),
        lastMoves: checkers.history.slice(-6).map((h) => h.notation),
        winner,
        statusLine:
          winner === "player"
            ? "Победа игрока в шашки"
            : winner === "jarvis"
              ? "Победа Джарвиса в шашки"
              : winner === "draw"
                ? "Ничья"
                : checkers.turn === "w"
                  ? "Ход белых (игрок)"
                  : "Ход чёрных (Джарвис)",
      };
    }
    const bs = battleshipFleetSummary(battleship);
    return {
      open: true,
      game: "battleship",
      phase:
        battleship.phase === "setup"
          ? "setup"
          : battleship.phase === "playing"
            ? "playing"
            : "finished",
      difficulty,
      mood,
      voiceEnabled: speakComments,
      turn: battleship.phase === "playing" ? battleship.turn : "none",
      moveCount: battleship.history.length,
      scoreSummary: bs.text,
      boardSummary: bs.safeBoardReport,
      lastMoves: battleship.history.slice(-6).map((h) => `${h.by === "player" ? "Игрок" : "Джарвис"} ${h.coord} (${h.result})`),
      winner: battleship.winner,
      statusLine:
        battleship.phase === "setup"
          ? "Расстановка кораблей перед боем"
          : battleship.winner === "player"
            ? "Флот Джарвиса потоплен! Победа игрока"
            : battleship.winner === "jarvis"
              ? "Победа флота Джарвиса"
              : battleship.turn === "player"
                ? "Ваш залп"
                : "Залп Джарвиса",
    };
  }, [activeGame, battleship, checkers, chess, difficulty, mood, speakComments]);

  useEffect(() => {
    gamesIpc.syncSnapshot(buildSnapshot());
  }, [buildSnapshot]);

  const requestLiveComment = useCallback(
    (
      partial: Omit<CommentRequest, "difficulty" | "mood" | "recentJarvisLines" | "speak"> & {
        difficulty?: Difficulty;
        mood?: PlayerMood;
      },
      allowSpeak = true,
    ) => {
      setLiveSpeech("");
      setStreamingSpeech(true);
      void gamesIpc
        .comment({
          ...partial,
          difficulty: partial.difficulty ?? difficulty,
          mood: partial.mood ?? mood,
          recentJarvisLines: recentJarvisLines.current,
          speak: speakComments && allowSpeak,
        })
        .then((id) => {
          activeReqId.current = Math.max(activeReqId.current, id);
        });
    },
    [difficulty, mood, speakComments],
  );

  // Start / reset a specific game
  const launchGame = useCallback(
    (
      game: BoardGameId,
      opts?: { diff?: Difficulty; nextMood?: PlayerMood; quietComment?: boolean },
    ) => {
      const useDiff = opts?.diff ?? difficulty;
      const useMood = opts?.nextMood ?? mood;
      if (opts?.diff) setDifficulty(opts.diff);
      if (opts?.nextMood) setMood(opts.nextMood);
      setActiveGame(game);
      setJarvisThinking(false);
      recentJarvisLines.current = [];
      setChatLog([]);

      if (game === "chess") {
        const fresh = createChessState();
        setChess(fresh);
        setChessUndo([]);
        setChessHint(null);
        if (!opts?.quietComment) {
          requestLiveComment({
            game: "chess",
            difficulty: useDiff,
            mood: useMood,
            trigger: "start",
            moveCount: 1,
            tacticalNotes: ["Начало новой шахматной партии, у игрока белые фигуры"],
            scoreSummary: "Начальная позиция",
            boardSummary: toFen(fresh),
          });
        }
      } else if (game === "checkers") {
        const fresh = createCheckersState();
        setCheckers(fresh);
        setCheckersUndo([]);
        setCheckersHint(null);
        if (!opts?.quietComment) {
          requestLiveComment({
            game: "checkers",
            difficulty: useDiff,
            mood: useMood,
            trigger: "start",
            moveCount: 1,
            tacticalNotes: ["Начало новой партии в русские шашки, у игрока белые"],
            scoreSummary: "По 12 шашек",
            boardSummary: checkersBoardText(fresh),
          });
        }
      } else {
        const fresh = createBattleshipState();
        setBattleship(fresh);
        setBsHint(null);
        setBsMissStreak(0);
        if (!opts?.quietComment) {
          requestLiveComment({
            game: "battleship",
            difficulty: useDiff,
            mood: useMood,
            trigger: "start",
            moveCount: 0,
            tacticalNotes: ["Игрок расставляет свой флот из 10 кораблей перед началом Морского боя"],
            scoreSummary: "По 10 кораблей",
            boardSummary: "Фаза расстановки кораблей",
          });
        }
      }
    },
    [difficulty, mood, requestLiveComment],
  );

  // ---------------- CHESS TURN EXECUTION ----------------
  const executePlayerChessMove = useCallback(
    async (
      move: ChessMove,
      fromRemoteAgent = false,
      baseState?: ChessState,
    ): Promise<{ summary: string; data: Record<string, unknown> }> => {
      const startSt = baseState ?? chess;
      setChessHint(null);
      setChessUndo((u) => [...u.slice(-24), startSt]);
      const afterPlayer = applyChessMove(startSt, move);
      setChess(afterPlayer);
      const pItem = afterPlayer.history.at(-1)!;

      if (pItem.checkmate) sfx.win();
      else if (pItem.check) sfx.check();
      else if (move.promotion) sfx.promote();
      else if (pItem.captured) sfx.capture();
      else sfx.move();

      if (afterPlayer.status !== "playing") {
        const w = afterPlayer.winner === "w" ? "player" : afterPlayer.winner === "b" ? "jarvis" : "draw";
        void gamesIpc.recordFinish("chess", w).then(setStats);
        const notes = describeChessExchange(startSt, afterPlayer, null);
        if (!fromRemoteAgent) {
          requestLiveComment({
            game: "chess",
            trigger: "finish",
            moveCount: afterPlayer.fullmove,
            playerMove: pItem.ru,
            tacticalNotes: notes,
            scoreSummary: materialBalanceSummary(afterPlayer),
            boardSummary: toFen(afterPlayer),
          });
        }
        return {
          summary: `Вы сыграли ${pItem.ru} — партия окончена (${w === "player" ? "победа игрока!" : "ничья"}).`,
          data: { playerMove: pItem.ru, fen: toFen(afterPlayer), status: afterPlayer.status, notes },
        };
      }

      // Jarvis's reply
      setJarvisThinking(true);
      await new Promise((r) => window.setTimeout(r, 340));
      const replyMove = chooseChessMove(afterPlayer, difficulty);
      setJarvisThinking(false);

      if (!replyMove) {
        return { summary: `Вы сыграли ${pItem.ru}.`, data: { playerMove: pItem.ru, fen: toFen(afterPlayer) } };
      }

      const afterJarvis = applyChessMove(afterPlayer, replyMove);
      setChess(afterJarvis);
      const jItem = afterJarvis.history.at(-1)!;

      if (jItem.checkmate) sfx.lose();
      else if (jItem.check) sfx.check();
      else if (replyMove.promotion) sfx.promote();
      else if (jItem.captured) sfx.capture();
      else sfx.move();

      const notes = describeChessExchange(startSt, afterPlayer, afterJarvis);
      if (afterJarvis.status !== "playing") {
        const w = afterJarvis.winner === "w" ? "player" : afterJarvis.winner === "b" ? "jarvis" : "draw";
        void gamesIpc.recordFinish("chess", w).then(setStats);
      }

      // Comment on every finish/critical/capture, plus ~65% of quiet moves so conversation feels alive and natural
      const notable =
        afterJarvis.status !== "playing" ||
        notes.length > 0 ||
        afterJarvis.fullmove <= 3 ||
        afterJarvis.fullmove % 2 === 0;

      if (!fromRemoteAgent && notable) {
        requestLiveComment({
          game: "chess",
          trigger:
            afterJarvis.status !== "playing"
              ? "finish"
              : jItem.check || pItem.check
                ? "critical"
                : jItem.captured || pItem.captured
                  ? "capture"
                  : "move",
          moveCount: afterJarvis.fullmove,
          playerMove: pItem.ru,
          jarvisMove: jItem.ru,
          tacticalNotes: notes,
          scoreSummary: materialBalanceSummary(afterJarvis),
          boardSummary: toFen(afterJarvis),
        });
      }

      return {
        summary: `Ход игрока: ${pItem.ru}. Ответ Джарвиса: ${jItem.ru}. ${materialBalanceSummary(afterJarvis)}`,
        data: {
          playerMove: pItem.ru,
          jarvisMove: jItem.ru,
          fen: toFen(afterJarvis),
          opening: afterJarvis.openingName,
          tacticalNotes: notes,
          status: afterJarvis.status,
        },
      };
    },
    [chess, difficulty, requestLiveComment],
  );

  // ---------------- CHECKERS TURN EXECUTION ----------------
  const executePlayerCheckersMove = useCallback(
    async (
      move: CheckersMove,
      fromRemoteAgent = false,
      baseState?: CheckersState,
    ): Promise<{ summary: string; data: Record<string, unknown> }> => {
      const startSt = baseState ?? checkers;
      setCheckersHint(null);
      setCheckersUndo((u) => [...u.slice(-24), startSt]);
      const afterPlayer = applyCheckersMove(startSt, move);
      setCheckers(afterPlayer);
      const pItem = afterPlayer.history.at(-1)!;

      if (afterPlayer.winner === "w") sfx.win();
      else if (move.promoted) sfx.promote();
      else if (move.captures.length > 0) sfx.capture();
      else sfx.move();

      if (afterPlayer.status !== "playing") {
        const w = afterPlayer.winner === "w" ? "player" : "draw";
        void gamesIpc.recordFinish("checkers", w).then(setStats);
        const notes = describeCheckersExchange(afterPlayer, null);
        if (!fromRemoteAgent) {
          requestLiveComment({
            game: "checkers",
            trigger: "finish",
            moveCount: afterPlayer.history.length,
            playerMove: pItem.notation,
            tacticalNotes: notes,
            scoreSummary: checkersScoreSummary(afterPlayer).text,
            boardSummary: checkersBoardText(afterPlayer),
          });
        }
        return {
          summary: `Ход ${pItem.notation} — партия завершена!`,
          data: { playerMove: pItem.notation, notes, status: afterPlayer.status },
        };
      }

      setJarvisThinking(true);
      await new Promise((r) => window.setTimeout(r, 340));
      const replyMove = chooseCheckersMove(afterPlayer, difficulty);
      setJarvisThinking(false);

      if (!replyMove) {
        return { summary: `Ход ${pItem.notation}.`, data: { playerMove: pItem.notation } };
      }

      const afterJarvis = applyCheckersMove(afterPlayer, replyMove);
      setCheckers(afterJarvis);
      const jItem = afterJarvis.history.at(-1)!;

      if (afterJarvis.winner === "b") sfx.lose();
      else if (replyMove.promoted) sfx.promote();
      else if (replyMove.captures.length > 0) sfx.capture();
      else sfx.move();

      const notes = describeCheckersExchange(afterPlayer, afterJarvis);
      if (afterJarvis.status !== "playing") {
        const w = afterJarvis.winner === "b" ? "jarvis" : "draw";
        void gamesIpc.recordFinish("checkers", w).then(setStats);
      }

      const sc = checkersScoreSummary(afterJarvis);
      const turnNo = Math.ceil(afterJarvis.history.length / 2);
      const notable = afterJarvis.status !== "playing" || notes.length > 0 || turnNo <= 2 || turnNo % 2 === 0;
      if (!fromRemoteAgent && notable) {
        requestLiveComment({
          game: "checkers",
          trigger: afterJarvis.status !== "playing" ? "finish" : notes.length > 0 ? "capture" : "move",
          moveCount: turnNo,
          playerMove: pItem.notation,
          jarvisMove: jItem.notation,
          tacticalNotes: notes,
          scoreSummary: sc.text,
          boardSummary: checkersBoardText(afterJarvis),
        });
      }

      return {
        summary: `Вы: ${pItem.notation}, Джарвис: ${jItem.notation}. ${sc.text}`,
        data: {
          playerMove: pItem.notation,
          jarvisMove: jItem.notation,
          score: sc.text,
          tacticalNotes: notes,
          status: afterJarvis.status,
        },
      };
    },
    [checkers, difficulty, requestLiveComment],
  );

  // ---------------- BATTLESHIP SHOT EXECUTION ----------------
  const executePlayerBattleshipShot = useCallback(
    async (
      cell: number,
      baseState?: BattleshipState,
      fromRemoteAgent = false,
    ): Promise<{ summary: string; data: Record<string, unknown> }> => {
      setBsHint(null);
      const startSt = baseState ?? battleship;
      const { state: afterPlayer, outcome } = firePlayerShot(startSt, cell);
      if (outcome.result === "repeat") {
        return { summary: `В сектор ${outcome.coord} уже стреляли — выбери свободную клетку.`, data: { repeat: true } };
      }

      setBattleship(afterPlayer);
      const notes: string[] = [];

      if (outcome.result === "sunk") {
        sfx.sunk();
        setBsMissStreak(0);
        notes.push(`Игрок выстрелом в ${outcome.coord} ПОТОПИЛ ${outcome.shipName} Джарвиса!`);
      } else if (outcome.result === "hit") {
        sfx.hit();
        setBsMissStreak(0);
        notes.push(`Игрок попал в корабль Джарвиса на ${outcome.coord} и стреляет снова!`);
      } else {
        sfx.splash();
      }

      if (afterPlayer.winner === "player") {
        sfx.win();
        void gamesIpc.recordFinish("battleship", "player").then(setStats);
        const sum = battleshipFleetSummary(afterPlayer);
        if (!fromRemoteAgent) {
          requestLiveComment({
            game: "battleship",
            trigger: "finish",
            moveCount: afterPlayer.history.length,
            playerMove: `${outcome.coord} — потоплен последний корабль!`,
            tacticalNotes: ["Игрок потопил весь флот Джарвиса и выиграл Морской бой!"],
            scoreSummary: sum.text,
            boardSummary: sum.safeBoardReport,
          });
        }
        return {
          summary: `${outcome.coord} — потоплен ${outcome.shipName}! Весь флот Джарвиса на дне, ваша победа!`,
          data: { playerShot: outcome, winner: "player" },
        };
      }

      // Player keeps shooting on hit/sunk!
      if (outcome.result === "hit" || outcome.result === "sunk") {
        const sum = battleshipFleetSummary(afterPlayer);
        if (!fromRemoteAgent) {
          requestLiveComment({
            game: "battleship",
            trigger: outcome.result === "sunk" ? "critical" : "capture",
            moveCount: afterPlayer.history.length,
            playerMove: `${outcome.coord} (${outcome.result === "sunk" ? "потопил" : "ранил"})`,
            tacticalNotes: notes,
            scoreSummary: sum.text,
            boardSummary: sum.safeBoardReport,
          });
        }
        return {
          summary: `${outcome.coord} — ${outcome.result === "sunk" ? `потоплен ${outcome.shipName}` : "ранен"}! Ваш повторный залп.`,
          data: { playerShot: outcome, turn: "player", fleet: sum.text },
        };
      }

      // Player missed -> Jarvis fires (and continues while hitting!)
      const nextStreak = bsMissStreak + 1;
      setBsMissStreak(nextStreak);
      if (nextStreak >= 3) {
        notes.push(`Игрок промахнулся уже ${nextStreak} раза подряд (${outcome.coord} — мимо)`);
      }

      setJarvisThinking(true);
      let cur = afterPlayer;
      const jarvisSalvo: string[] = [];

      while (cur.phase === "playing" && cur.turn === "jarvis") {
        await new Promise((r) => window.setTimeout(r, jarvisSalvo.length === 0 ? 380 : 680));
        const remSizes = cur.playerShips.filter((s) => !s.sunk).map((s) => s.size);
        const shotCell = chooseBattleshipShot(cur.jarvisShots, remSizes, difficulty);
        const step = fireJarvisShot(cur, shotCell);
        cur = step.state;
        setBattleship(cur);

        if (step.outcome.result === "sunk") {
          sfx.sunk();
          jarvisSalvo.push(`${step.outcome.coord} (потопил ${step.outcome.shipName}!)`);
          notes.push(`Джарвис потопил ${step.outcome.shipName} игрока на ${step.outcome.coord}!`);
        } else if (step.outcome.result === "hit") {
          sfx.hit();
          jarvisSalvo.push(`${step.outcome.coord} (ранил)`);
          notes.push(`Джарвис подбил корабль игрока в квадрате ${step.outcome.coord}`);
        } else {
          sfx.splash();
          jarvisSalvo.push(`${step.outcome.coord} (мимо)`);
        }
      }
      setJarvisThinking(false);

      if (cur.winner === "jarvis") {
        sfx.lose();
        void gamesIpc.recordFinish("battleship", "jarvis").then(setStats);
        notes.push("Джарвис потопил весь флот игрока и победил в Морском бою!");
      }

      const sum = battleshipFleetSummary(cur);
      const jSummary = jarvisSalvo.join(" → ");
      const shouldComment =
        cur.winner !== null ||
        notes.length > 0 ||
        cur.history.length <= 4 ||
        cur.history.length % 6 === 0;

      if (!fromRemoteAgent && shouldComment) {
        requestLiveComment({
          game: "battleship",
          trigger: cur.winner ? "finish" : notes.length > 0 ? "capture" : "move",
          moveCount: cur.history.length,
          playerMove: `${outcome.coord} (мимо)`,
          jarvisMove: jSummary,
          tacticalNotes: notes,
          scoreSummary: sum.text,
          boardSummary: sum.safeBoardReport,
        });
      }

      return {
        summary: `Ваш залп ${outcome.coord}: мимо. Залп Джарвиса: ${jSummary}. ${sum.text}`,
        data: {
          playerShot: outcome,
          jarvisSalvo,
          fleet: sum.text,
          winner: cur.winner,
        },
      };
    },
    [battleship, bsMissStreak, difficulty, requestLiveComment],
  );

  // ---------------- REMOTE COMMAND BRIDGE (VOICE / AGENT `game_play`) ----------------
  const handleRemoteCmd = useCallback(
    async (cmd: RemoteGameCommand) => {
      if (cmd.action === "open" || cmd.action === "new_game") {
        const g =
          cmd.game === "chess" || cmd.game === "checkers" || cmd.game === "battleship"
            ? cmd.game
            : activeGame;
        const d =
          cmd.difficulty === "easy" || cmd.difficulty === "medium" || cmd.difficulty === "hard"
            ? cmd.difficulty
            : undefined;
        const m =
          cmd.mood === "auto" || cmd.mood === "joker" || cmd.mood === "business" || cmd.mood === "rival"
            ? cmd.mood
            : undefined;
        if (g) {
          launchGame(g, { diff: d, nextMood: m, quietComment: true });
          gamesIpc.replyCmd({
            requestId: cmd.requestId,
            ok: true,
            summary: `Открыта игра «${GAME_META[g].title}» (${DIFFICULTY_LABELS[d ?? difficulty].label}, манера: ${MOOD_LABELS[m ?? mood].label})`,
            data: { game: g, difficulty: d ?? difficulty, mood: m ?? mood },
          });
        } else {
          if (m) setMood(m);
          if (d) setDifficulty(d);
          setActiveGame(null);
          gamesIpc.replyCmd({
            requestId: cmd.requestId,
            ok: true,
            summary: "Открыт игровой клуб Джарвиса: шахматы, русские шашки и морской бой.",
          });
        }
        return;
      }

      if (cmd.action === "set_mood") {
        const m =
          cmd.mood === "auto" || cmd.mood === "joker" || cmd.mood === "business" || cmd.mood === "rival"
            ? cmd.mood
            : "auto";
        setMood(m);
        triggerEmote(m === "business" ? "determined" : m === "joker" ? "laugh" : "wink");
        gamesIpc.replyCmd({
          requestId: cmd.requestId,
          ok: true,
          summary: `Манера Джарвиса за столом переключена: ${MOOD_LABELS[m].label}`,
          data: { mood: m },
        });
        return;
      }

      if (cmd.action === "resign") {
        if (!activeGame) {
          gamesIpc.replyCmd({ requestId: cmd.requestId, ok: false, summary: "Сейчас нет активной партии." });
          return;
        }
        if (activeGame === "chess") setChess((s) => ({ ...s, status: "resigned", winner: "b" }));
        else if (activeGame === "checkers") setCheckers((s) => ({ ...s, status: "resigned", winner: "b" }));
        else setBattleship((s) => ({ ...s, phase: "resigned", winner: "jarvis" }));
        void gamesIpc.recordFinish(activeGame, "jarvis").then(setStats);
        gamesIpc.replyCmd({
          requestId: cmd.requestId,
          ok: true,
          summary: `Партия в «${GAME_META[activeGame].title}» завершена сдачей игрока.`,
        });
        return;
      }

      if (cmd.action === "hint") {
        if (activeGame === "chess") {
          const h = hintChessMove(chess);
          if (h) setChessHint(h.move);
          gamesIpc.replyCmd({
            requestId: cmd.requestId,
            ok: Boolean(h),
            summary: h?.text ?? "Ходов нет.",
          });
        } else if (activeGame === "checkers") {
          const h = hintCheckersMove(checkers);
          if (h) setCheckersHint(h.move);
          gamesIpc.replyCmd({
            requestId: cmd.requestId,
            ok: Boolean(h),
            summary: h?.text ?? "Ходов нет.",
          });
        } else if (activeGame === "battleship") {
          const st = battleship.phase === "setup" ? startBattleshipMatch(battleship) : battleship;
          if (battleship.phase === "setup") setBattleship(st);
          const h = hintBattleshipShot(st);
          if (h) setBsHint(h.cell);
          gamesIpc.replyCmd({
            requestId: cmd.requestId,
            ok: Boolean(h),
            summary: h?.text ?? "Бой уже окончен.",
          });
        } else {
          gamesIpc.replyCmd({
            requestId: cmd.requestId,
            ok: false,
            summary: "Сначала выберите игру: шахматы, шашки или морской бой.",
          });
        }
        return;
      }

      if (cmd.action === "move") {
        const raw = (cmd.moveStr ?? "").trim();
        const targetGame =
          cmd.game === "chess" || cmd.game === "checkers" || cmd.game === "battleship"
            ? cmd.game
            : activeGame;

        if (!targetGame) {
          gamesIpc.replyCmd({
            requestId: cmd.requestId,
            ok: false,
            summary: "Сначала открой игру (шахматы, шашки или морской бой).",
          });
          return;
        }
        if (targetGame !== activeGame) {
          launchGame(targetGame, { quietComment: true });
        }

        if (targetGame === "chess") {
          const base = targetGame === activeGame ? chess : createChessState();
          const parsed = parseSpokenChessMove(base, raw);
          if (!parsed) {
            gamesIpc.replyCmd({
              requestId: cmd.requestId,
              ok: false,
              summary: `Не удалось распознать легальный шахматный ход «${raw}» в текущей позиции.`,
              data: { fen: toFen(base) },
            });
            return;
          }
          const out = await executePlayerChessMove(parsed, true, base);
          gamesIpc.replyCmd({ requestId: cmd.requestId, ok: true, summary: out.summary, data: out.data });
        } else if (targetGame === "checkers") {
          const base = targetGame === activeGame ? checkers : createCheckersState();
          const parsed = parseSpokenCheckersMove(base, raw);
          if (!parsed) {
            gamesIpc.replyCmd({
              requestId: cmd.requestId,
              ok: false,
              summary: `Ход «${raw}» недопустим по правилам русских шашек (проверьте обязательный бой).`,
            });
            return;
          }
          const out = await executePlayerCheckersMove(parsed, true, base);
          gamesIpc.replyCmd({ requestId: cmd.requestId, ok: true, summary: out.summary, data: out.data });
        } else {
          let base = targetGame === activeGame ? battleship : createBattleshipState();
          if (base.phase === "setup") {
            base = startBattleshipMatch(base);
            setBattleship(base);
          }
          const cell = parseSpokenBattleshipShot(raw);
          if (cell === null) {
            gamesIpc.replyCmd({
              requestId: cmd.requestId,
              ok: false,
              summary: `Не понял координаты выстрела «${raw}» (ожидается А1..К10).`,
            });
            return;
          }
          const out = await executePlayerBattleshipShot(cell, base, true);
          gamesIpc.replyCmd({ requestId: cmd.requestId, ok: true, summary: out.summary, data: out.data });
        }
      }
    },
    [
      activeGame,
      battleship,
      checkers,
      chess,
      difficulty,
      executePlayerBattleshipShot,
      executePlayerCheckersMove,
      executePlayerChessMove,
      launchGame,
      mood,
      triggerEmote,
    ],
  );

  const remoteHandlerRef = useRef(handleRemoteCmd);
  remoteHandlerRef.current = handleRemoteCmd;

  useEffect(() => {
    void gamesIpc.ready().then((bootCmd) => {
      if (bootCmd) {
        void remoteHandlerRef.current(bootCmd);
      } else if (init.game) {
        launchGame(init.game, { diff: init.diff, nextMood: init.mood });
      }
    });
    const un = gamesIpc.onRemoteCmd((cmd) => {
      void remoteHandlerRef.current(cmd);
    });
    return () => {
      void un.then((f) => f());
    };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ---------------- TABLE CHAT / INLINE MOVE SUBMIT ----------------
  const sendTableInput = async (rawText?: string) => {
    const text = (rawText ?? chatInput).trim();
    if (!text) return;
    if (!rawText) setChatInput("");

    // Check if the player typed a direct move on their turn!
    if (activeGame === "chess" && chess.status === "playing" && chess.turn === "w" && !jarvisThinking) {
      const mv = parseSpokenChessMove(chess, text);
      if (mv && text.split(/\s+/).length <= 3) {
        pushLog("player", `Ход: ${text}`);
        await executePlayerChessMove(mv, false);
        return;
      }
    } else if (activeGame === "checkers" && checkers.status === "playing" && checkers.turn === "w" && !jarvisThinking) {
      const mv = parseSpokenCheckersMove(checkers, text);
      if (mv && text.split(/\s+/).length <= 3) {
        pushLog("player", `Ход: ${text}`);
        await executePlayerCheckersMove(mv, false);
        return;
      }
    } else if (activeGame === "battleship" && battleship.phase === "playing" && battleship.turn === "player" && !jarvisThinking) {
      const cell = parseSpokenBattleshipShot(text);
      if (cell !== null && text.split(/\s+/).length <= 2) {
        pushLog("player", `Залп: ${text.toUpperCase()}`);
        await executePlayerBattleshipShot(cell);
        return;
      }
    }

    // Free conversation at the game table!
    pushLog("player", text);
    const snap = buildSnapshot();
    requestLiveComment({
      game: activeGame ?? "chess",
      trigger: "chat",
      moveCount: snap.moveCount,
      tacticalNotes: [],
      scoreSummary: snap.scoreSummary,
      boardSummary: snap.boardSummary,
      playerSay: text,
    });
  };

  const handleMoodPick = (next: PlayerMood) => {
    setMood(next);
    triggerEmote(next === "business" ? "determined" : next === "joker" ? "laugh" : "wink");
    if (activeGame) {
      const snap = buildSnapshot();
      requestLiveComment({
        game: activeGame,
        mood: next,
        trigger: "mood_change",
        moveCount: snap.moveCount,
        tacticalNotes: [`Игрок переключил твою манеру за столом на «${MOOD_LABELS[next].label}»`],
        scoreSummary: snap.scoreSummary,
        boardSummary: snap.boardSummary,
      });
    }
  };

  const handleHint = () => {
    if (activeGame === "chess" && chess.status === "playing") {
      const h = hintChessMove(chess);
      if (h) {
        setChessHint(h.move);
        triggerEmote("wink");
        requestLiveComment({
          game: "chess",
          trigger: "hint",
          moveCount: chess.fullmove,
          tacticalNotes: [`Игрок попросил подсказку. Лучшая идея: ${h.text}`],
          scoreSummary: materialBalanceSummary(chess),
          boardSummary: toFen(chess),
        });
      }
    } else if (activeGame === "checkers" && checkers.status === "playing") {
      const h = hintCheckersMove(checkers);
      if (h) {
        setCheckersHint(h.move);
        triggerEmote("wink");
        requestLiveComment({
          game: "checkers",
          trigger: "hint",
          moveCount: checkers.history.length,
          tacticalNotes: [`Игрок попросил подсказку. Лучшая идея: ${h.text}`],
          scoreSummary: checkersScoreSummary(checkers).text,
          boardSummary: checkersBoardText(checkers),
        });
      }
    } else if (activeGame === "battleship" && battleship.phase === "playing") {
      const h = hintBattleshipShot(battleship);
      if (h) {
        setBsHint(h.cell);
        triggerEmote("wink");
        const sum = battleshipFleetSummary(battleship);
        requestLiveComment({
          game: "battleship",
          trigger: "hint",
          moveCount: battleship.history.length,
          tacticalNotes: [`Игрок попросил совет радара. Лучший сектор: ${h.text}`],
          scoreSummary: sum.text,
          boardSummary: sum.safeBoardReport,
        });
      }
    }
  };

  const handleUndo = () => {
    if (activeGame === "chess" && chessUndo.length > 0 && !jarvisThinking) {
      const prev = chessUndo.at(-1)!;
      setChessUndo((u) => u.slice(0, -1));
      setChess(prev);
      setChessHint(null);
      triggerEmote("squint");
    } else if (activeGame === "checkers" && checkersUndo.length > 0 && !jarvisThinking) {
      const prev = checkersUndo.at(-1)!;
      setCheckersUndo((u) => u.slice(0, -1));
      setCheckers(prev);
      setCheckersHint(null);
      triggerEmote("squint");
    }
  };

  const toggleSpeak = () => {
    const next = !speakComments;
    setSpeakComments(next);
    if (!next) void stopTts();
    try {
      localStorage.setItem("jarvis.games.speak", next ? "1" : "0");
    } catch {
      /* ignore */
    }
  };

  const toggleSfx = () => {
    const next = !sfxOn;
    setSfxOn(next);
    setSfxMuted(!next);
    try {
      localStorage.setItem("jarvis.games.sfx", next ? "1" : "0");
    } catch {
      /* ignore */
    }
  };

  const eyeMood = ttsSpeaking
    ? "speaking"
    : voiceState === "listening" || voiceState === "hearing"
      ? "listening"
      : jarvisThinking || streamingSpeech
        ? "thinking"
        : "attentive";

  const snap = buildSnapshot();
  const isFinished = snap.phase === "finished";

  return (
    <div className="games-app" style={skinCssVars(skin)}>
      <TitleBar />

      {/* Top Navigation Chrome */}
      <header className="gm-topbar" data-tauri-drag-region>
        <div className="gm-brand" data-tauri-drag-region>
          {activeGame ? (
            <button type="button" className="gm-back-btn" onClick={() => setActiveGame(null)}>
              <Icon name="chevronLeft" size={14} />
              <span>Клуб игр</span>
            </button>
          ) : (
            <div className="gm-brand-logo" data-tauri-drag-region>
              <span className="gm-logo-pill" aria-hidden="true">
                <i />
                <i />
              </span>
              <b>jarvis</b>
              <span>игровой клуб</span>
            </div>
          )}

          {activeGame && (
            <div className="gm-active-title" data-tauri-drag-region>
              <img src={GAME_ART[activeGame]} alt="" draggable={false} />
              <div>
                <b>{GAME_META[activeGame].title}</b>
                <span>{ snap.statusLine }</span>
              </div>
            </div>
          )}
        </div>

        <div className="gm-top-controls">
          <div className="gm-seg" role="group" aria-label="Сложность ИИ">
            {(Object.keys(DIFFICULTY_LABELS) as Difficulty[]).map((d) => (
              <button
                key={d}
                type="button"
                className={`gm-seg-btn${difficulty === d ? " is-on" : ""}`}
                title={DIFFICULTY_LABELS[d].sub}
                onClick={() => setDifficulty(d)}
              >
                {DIFFICULTY_LABELS[d].label}
              </button>
            ))}
          </div>

          {activeGame && (
            <div className="gm-action-group">
              {(activeGame === "chess" || activeGame === "checkers") && (
                <>
                  <button
                    type="button"
                    className="gm-btn"
                    disabled={
                      jarvisThinking ||
                      (activeGame === "chess" ? chessUndo.length === 0 : checkersUndo.length === 0)
                    }
                    onClick={handleUndo}
                    title="Вернуть ход назад"
                  >
                    Назад
                  </button>
                  <button
                    type="button"
                    className="gm-btn"
                    onClick={() => setFlipped((f) => !f)}
                    title="Перевернуть доску"
                  >
                    Развернуть
                  </button>
                </>
              )}
              {snap.phase === "playing" && (
                <button
                  type="button"
                  className="gm-btn"
                  disabled={jarvisThinking}
                  onClick={handleHint}
                >
                  Подсказка
                </button>
              )}
              <button
                type="button"
                className="gm-btn is-primary"
                onClick={() => launchGame(activeGame)}
              >
                Новая партия
              </button>
            </div>
          )}
        </div>
      </header>

      {/* Main Split Stage */}
      <div className="gm-stage">
        <main className="gm-left">
          {!activeGame ? (
            <section className="gm-lobby">
              <div className="gm-lobby-head">
                <h1>Во что сыграем сегодня?</h1>
                <p>
                  Живые партии один на один со мной: я вижу позицию, комментирую ходы без заготовленных скриптов,
                  могу шутить и смеяться или вести строгую гроссмейстерскую борьбу.
                </p>
              </div>

              <div className="gm-lobby-cards">
                {(["chess", "checkers", "battleship"] as BoardGameId[]).map((id) => {
                  const meta = GAME_META[id];
                  const st = stats[id];
                  return (
                    <button
                      key={id}
                      type="button"
                      className="gm-card"
                      onClick={() => launchGame(id)}
                    >
                      <div className="gm-card-top">
                        <span className="gm-card-badge">{meta.badge}</span>
                        <img className="gm-card-art" src={GAME_ART[id]} alt="" draggable={false} />
                      </div>
                      <h2>{meta.title}</h2>
                      <span className="gm-card-sub">{meta.subtitle}</span>
                      <p>{meta.description}</p>
                      <div className="gm-card-foot">
                        <span className="gm-score-chip">
                          Счёт {st.wins}:{st.losses}
                          {st.draws > 0 ? ` · ничьих ${st.draws}` : ""}
                        </span>
                        <span className="gm-play-cta">
                          Играть
                          <Icon name="chevron" size={13} />
                        </span>
                      </div>
                    </button>
                  );
                })}
              </div>
            </section>
          ) : (
            <section className="gm-arena">
              {activeGame === "chess" && (
                <ChessBoard
                  state={chess}
                  flipped={flipped}
                  disabled={jarvisThinking}
                  hintMove={chessHint}
                  onMove={(m) => void executePlayerChessMove(m)}
                />
              )}
              {activeGame === "checkers" && (
                <CheckersBoard
                  state={checkers}
                  flipped={flipped}
                  disabled={jarvisThinking}
                  hintMove={checkersHint}
                  onMove={(m) => void executePlayerCheckersMove(m)}
                />
              )}
              {activeGame === "battleship" && (
                <BattleshipBoard
                  state={battleship}
                  disabled={jarvisThinking}
                  hintCell={bsHint}
                  onShuffleFleet={() =>
                    setBattleship((s) => ({ ...s, playerShips: randomFleet() }))
                  }
                  onUpdateFleet={(playerShips) =>
                    setBattleship((s) => ({ ...s, playerShips }))
                  }
                  onStartBattle={() => {
                    const started = startBattleshipMatch(battleship);
                    setBattleship(started);
                    const sum = battleshipFleetSummary(started);
                    requestLiveComment({
                      game: "battleship",
                      trigger: "start",
                      moveCount: 1,
                      tacticalNotes: ["Корабли расставлены! Морской бой начался, первый выстрел за игроком"],
                      scoreSummary: sum.text,
                      boardSummary: sum.safeBoardReport,
                    });
                  }}
                  onFire={(cell) => void executePlayerBattleshipShot(cell)}
                />
              )}

              {isFinished && (
                <div className="gm-finish-banner">
                  <img
                    src={snap.winner === "player" ? ART.trophy : GAME_ART[activeGame]}
                    alt=""
                    draggable={false}
                  />
                  <div>
                    <b>
                      {snap.winner === "player"
                        ? "Победа за вами!"
                        : snap.winner === "jarvis"
                          ? "Партия за Джарвисом"
                          : "Боевая ничья"}
                    </b>
                    <span>{snap.statusLine} · Общий счёт {stats[activeGame].wins}:{stats[activeGame].losses}</span>
                  </div>
                  <button
                    type="button"
                    className="gm-btn is-primary"
                    onClick={() => launchGame(activeGame)}
                  >
                    Реванш
                  </button>
                </div>
              )}
            </section>
          )}
        </main>

        {/* Right Side: JARVIS Live Presence, Persona Switcher & Table Conversation */}
        <aside className="gm-jarvis-panel" aria-label="Джарвис за игровым столом">
          <div className="gm-presence-card">
            <div className="gm-presence-top">
              <div
                className={`gm-eyes-pod${ttsSpeaking ? " is-speaking" : jarvisThinking ? " is-thinking" : ""}`}
                title="Двойной клик по глазам Джарвиса"
              >
                <Eyes
                  mood={eyeMood}
                  level={outputLevel}
                  scale={1.35}
                  emote={faceEmote}
                  emoteSeq={faceEmoteSeq}
                  onBoop={() => triggerEmote("boop")}
                />
              </div>
              <div className="gm-presence-meta">
                <div className="gm-presence-name">
                  <b>Джарвис</b>
                  <span className="gm-live-dot" />
                </div>
                <span className="gm-presence-sub">
                  {ttsSpeaking
                    ? "Говорит вслух…"
                    : jarvisThinking
                      ? "Считает варианты…"
                      : streamingSpeech
                        ? "Формулирует мысль…"
                        : MOOD_LABELS[mood].desc}
                </span>
              </div>
              <div className="gm-audio-toggles">
                <button
                  type="button"
                  className={`gm-icon-btn${speakComments ? " is-on" : ""}`}
                  title={speakComments ? "Голосовые реплики Джарвиса включены" : "Включить голосовую озвучку реплик"}
                  onClick={toggleSpeak}
                >
                  <Icon name={speakComments ? "volume" : "muted"} size={15} />
                </button>
                <button
                  type="button"
                  className={`gm-icon-btn${sfxOn ? " is-on" : ""}`}
                  title={sfxOn ? "Звуки доски и фигур включены" : "Включить звуки фигур"}
                  onClick={toggleSfx}
                >
                  <Icon name="bolt" size={14} />
                </button>
                {isTauri() && (
                  <button
                    type="button"
                    className={`gm-icon-btn${voiceState === "listening" || voiceState === "hearing" ? " is-mic-live" : ""}`}
                    title="Голосовой микрофон Джарвиса"
                    onClick={() => void toggleVoice()}
                  >
                    <Icon name="mic" size={14} />
                  </button>
                )}
              </div>
            </div>

            {/* Player Manner Selector */}
            <div className="gm-mood-bar" role="radiogroup" aria-label="Характер Джарвиса за игрой">
              {(Object.keys(MOOD_LABELS) as PlayerMood[]).map((m) => (
                <button
                  key={m}
                  type="button"
                  role="radio"
                  aria-checked={mood === m}
                  className={`gm-mood-pill${mood === m ? " is-on" : ""}`}
                  title={MOOD_LABELS[m].desc}
                  onClick={() => handleMoodPick(m)}
                >
                  {MOOD_LABELS[m].label}
                </button>
              ))}
            </div>

            {/* Live Speech Bubble */}
            <div className={`gm-speech-bubble${streamingSpeech ? " is-streaming" : ""}`}>
              <p>
                {liveSpeech || "Смотрю на позицию…"}
                {streamingSpeech && <span className="gm-caret" aria-hidden="true" />}
              </p>
            </div>
          </div>

          {/* Move Notation + Table Conversation Log */}
          <div className="gm-table-feed">
            {activeGame && snap.lastMoves.length > 0 && (
              <div className="gm-moves-strip">
                <span className="gm-moves-label">Ходы:</span>
                <div className="gm-moves-chips">
                  {snap.lastMoves.map((mv, idx) => (
                    <span key={idx} className="gm-move-chip">
                      {mv}
                    </span>
                  ))}
                </div>
              </div>
            )}

            <div className="gm-chat-scroll">
              {chatLog.length === 0 ? (
                <div className="gm-chat-empty">
                  {activeGame
                    ? GAME_META[activeGame].rulesHint
                    : "Скажите Джарвису пару слов, подколите его за ходом или попросите разобрать позицию."}
                </div>
              ) : (
                chatLog.map((item) => (
                  <div key={item.id} className={`gm-chat-line is-${item.who}`}>
                    <small>{item.who === "jarvis" ? "Джарвис" : "Вы"}</small>
                    <span>{item.text}</span>
                  </div>
                ))
              )}
              <div ref={logEndRef} />
            </div>

            <div className="gm-banter-row">
              {BANTER_CHIPS.map((c) => (
                <button
                  key={c.label}
                  type="button"
                  className="gm-banter-chip"
                  disabled={streamingSpeech}
                  onClick={() => void sendTableInput(c.say)}
                >
                  {c.label}
                </button>
              ))}
            </div>

            <form
              className="gm-chat-form"
              onSubmit={(e) => {
                e.preventDefault();
                void sendTableInput();
              }}
            >
              <input
                value={chatInput}
                onChange={(e) => setChatInput(e.target.value)}
                placeholder={
                  activeGame === "battleship"
                    ? "Реплика Джарвису или залп (Б5, К10)…"
                    : activeGame
                      ? "Скажи Джарвису или введи ход (e2e4, Кf3)…"
                      : "Напиши Джарвису за столом…"
                }
              />
              <button type="submit" className="gm-send-btn" disabled={!chatInput.trim()} aria-label="Отправить">
                <Icon name="send" size={15} />
              </button>
            </form>
          </div>
        </aside>
      </div>
    </div>
  );
}
