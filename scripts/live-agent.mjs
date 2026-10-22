// Live agent runs against the configured provider: sends each prompt to the
// running app (`--ask … --quiet`) and prints the transcript the agent stored:
// tool calls with their verified outcomes, the final answer, steps, timing.
//
//   node scripts/live-agent.mjs <path-to-jarvis.exe> "prompt 1" "prompt 2" …

import { DatabaseSync } from "node:sqlite";
import { execFileSync } from "node:child_process";
import path from "node:path";

const [exe, ...prompts] = process.argv.slice(2);
const db = new DatabaseSync(path.join(process.env.LOCALAPPDATA, "dev.jarvis.desktop", "jarvis.db"));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const text = (m) => (m.parts ?? []).filter((p) => p.type === "text").map((p) => p.text).join("");

for (const prompt of prompts) {
  const started = new Date().toISOString();
  execFileSync(exe, ["--ask", prompt, "--quiet"]);
  let task;
  for (let i = 0; i < 600; i++) {
    await sleep(250);
    task = db.prepare("SELECT * FROM tasks WHERE request = ? AND started_at >= ? ORDER BY started_at DESC LIMIT 1").get(prompt, started);
    if (task && task.status !== "running") break;
  }
  console.log(`\n▶ ${prompt}`);
  if (!task || task.status === "running") {
    console.log("  (не завершилось за 150 с)");
    continue;
  }
  const msgs = db
    .prepare("SELECT payload, tool_title, tool_ok FROM messages WHERE conversation_id = ? AND created_at >= ? ORDER BY id")
    .all(task.conversation_id, started);
  for (const row of msgs) {
    const m = JSON.parse(row.payload);
    if (m.role === "assistant" && m.tool_calls?.length) {
      for (const c of m.tool_calls) console.log(`  → ${c.name} ${c.arguments}`);
    } else if (m.role === "tool") {
      let summary = text(m);
      try {
        const j = JSON.parse(summary);
        summary = j.summary ?? j.error ?? summary;
      } catch {}
      console.log(`    ${row.tool_ok ? "✓" : "✗"} ${row.tool_title}: ${String(summary).slice(0, 160)}`);
    } else if (m.role === "assistant" && text(m).trim()) {
      console.log(`  ◀ ${text(m).trim().replace(/\n+/g, " ").slice(0, 400)}`);
    }
  }
  const ms = Date.parse(task.finished_at) - Date.parse(task.started_at);
  console.log(`  [${task.status}, шагов: ${task.steps}, ${(ms / 1000).toFixed(1)} с${task.error ? `, ошибка: ${task.error}` : ""}]`);
}
