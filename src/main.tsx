import { StrictMode, lazy, Suspense } from "react";
import { createRoot } from "react-dom/client";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import "@fontsource-variable/onest";
import "@fontsource-variable/jetbrains-mono";
import "./styles/tokens.css";
import "./styles/base.css";

const label = getCurrentWebviewWindow().label;
const root = createRoot(document.getElementById("root")!);

if (label === "areapick") {
  const AreaPick = lazy(() => import("./areapick/AreaPickApp"));
  document.body.classList.add("areapick-window");
  root.render(
    <StrictMode>
      <Suspense fallback={null}>
        <AreaPick />
      </Suspense>
    </StrictMode>,
  );
} else if (label === "overlay") {
  const Overlay = lazy(() => import("./overlay/OverlayApp"));
  document.body.classList.add("overlay-window");
  root.render(
    <StrictMode>
      <Suspense fallback={null}>
        <Overlay />
      </Suspense>
    </StrictMode>,
  );
} else if (label === "reset") {
  const Reset = lazy(() => import("./mentor/ResetApp"));
  document.body.classList.add("reset-window");
  root.render(
    <StrictMode>
      <Suspense fallback={null}>
        <Reset />
      </Suspense>
    </StrictMode>,
  );
} else if (label === "planner") {
  const Planner = lazy(() => import("./planner/PlannerApp"));
  document.body.classList.add("planner-window");
  root.render(
    <StrictMode>
      <Suspense fallback={null}>
        <Planner />
      </Suspense>
    </StrictMode>,
  );
} else if (label === "welcome") {
  const Welcome = lazy(() => import("./welcome/WelcomeApp"));
  document.body.classList.add("welcome-window");
  root.render(
    <StrictMode>
      <Suspense fallback={null}>
        <Welcome />
      </Suspense>
    </StrictMode>,
  );
} else if (label === "settings") {
  const Settings = lazy(() => import("./settings/SettingsApp"));
  document.body.classList.add("settings-window");
  root.render(
    <StrictMode>
      <Suspense fallback={null}>
        <Settings />
      </Suspense>
    </StrictMode>,
  );
} else {
  document.body.classList.add("island-window");
  void Promise.all([import("./island/Island"), import("./store/system"), import("./store/agent"), import("./store/prefs"), import("./store/jobs"), import("./store/voice"), import("./store/files"), import("./store/reminders"), import("./store/stopwatch"), import("./store/emote"), import("./store/clips"), import("./store/mentor"), import("./store/agenda"), import("./store/mcpPrompt"), import("./store/area")]).then(
    async ([{ Island }, system, agent, prefs, jobs, voice, files, reminders, stopwatch, emotes, clips, mentor, agenda, mcpPrompt, area]) => {
      root.render(
        <StrictMode>
          <Island />
        </StrictMode>,
      );
      mentor.watchMoments(() => emotes.emote("star"));
      await Promise.allSettled([prefs.startPrefsFeed(),jobs.startJobsFeed(),voice.startVoiceFeed(),system.startSystemFeeds(),agent.startAgentFeed(),files.startFilesFeed(),reminders.startRemindersFeed(),stopwatch.startStopwatchFeed(),emotes.startEmoteFeed(),clips.startClipsFeed(),mentor.startMentorFeed(),agenda.startAgendaFeed(),mcpPrompt.startMcpPromptFeed(),area.startAreaFeed()]);
    },
  );
}
