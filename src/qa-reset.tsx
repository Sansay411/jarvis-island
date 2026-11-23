// Visual QA for «Перезагрузка» (`qa/reset.html`): `?reason=budget|strict|voluntary`,
// `?screen=feed|games|workout|breath|step|done`, `?plan=1` for an if-then plan.

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource-variable/onest";
import "@fontsource-variable/jetbrains-mono";
import "./styles/tokens.css";
import "./styles/base.css";
import ResetApp from "./mentor/ResetApp";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ResetApp />
  </StrictMode>,
);
