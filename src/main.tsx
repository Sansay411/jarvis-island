import { StrictMode, lazy, Suspense } from "react";
import { createRoot } from "react-dom/client";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import "@fontsource-variable/onest";
import "@fontsource-variable/jetbrains-mono";
import "./styles/tokens.css";
import "./styles/base.css";

const label = getCurrentWebviewWindow().label;
const root = createRoot(document.getElementById("root")!);

if (label === "settings") {
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
  void Promise.all([import("./island/Island"), import("./store/system"), import("./store/agent"), import("./store/prefs"), import("./store/jobs"), import("./store/voice"), import("./store/files"), import("./store/reminders")]).then(
    async ([{ Island }, system, agent, prefs, jobs, voice, files, reminders]) => {
      root.render(
        <StrictMode>
          <Island />
        </StrictMode>,
      );
      await Promise.allSettled([prefs.startPrefsFeed(),jobs.startJobsFeed(),voice.startVoiceFeed(),system.startSystemFeeds(),agent.startAgentFeed(),files.startFilesFeed(),reminders.startRemindersFeed()]);
    },
  );
}
