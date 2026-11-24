// Visual QA for JARVIS Games Club (`qa/games.html`):
// `?game=chess|checkers|battleship`, `?difficulty=easy|medium|hard`, `?mood=auto|joker|rival|business`.

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource-variable/onest";
import "@fontsource-variable/jetbrains-mono";
import "./styles/tokens.css";
import "./styles/base.css";
import GamesApp from "./games/GamesApp";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <GamesApp />
  </StrictMode>,
);
