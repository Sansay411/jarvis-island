// The 3D icons of «Перезагрузка» and of growth moments on the island:
// soft clay in the installer's language (light grey, amber, JARVIS's eyes
// on the rocket). Generated with Codex, 256 px WebP with alpha.

import breath from "./art/breath.webp";
import feed from "./art/feed.webp";
import game from "./art/game.webp";
import level from "./art/level.webp";
import sprint from "./art/sprint.webp";
import step from "./art/step.webp";
import streak from "./art/streak.webp";
import trophy from "./art/trophy.webp";
import words from "./art/words.webp";
import workout from "./art/workout.webp";

export const ART = { feed, games: game, workout, step, breath, sprint, words, streak, trophy, level } as const;
export type ArtName = keyof typeof ART;
