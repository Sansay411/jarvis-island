// End-to-end test of the agent loop in the running app, against the scripted
// mock LLM (scripts/mock-llm.mjs). Real tools run for real; only the model is
// simulated. Settings are backed up and restored.
//
//   node scripts/e2e-agent.mjs <path-to-jarvis.exe> [port]

import { DatabaseSync } from "node:sqlite";
import { execFileSync, spawn } from "node:child_process";
import path from "node:path";
import fs from "node:fs";
import os from "node:os";

const exe = process.argv[2];
const port = Number(process.argv[3] ?? 18080);
const dbPath = path.join(process.env.LOCALAPPDATA, "dev.jarvis.desktop", "jarvis.db");
const logPath = path.join(os.tmpdir(), `jarvis-mock-${Date.now()}.jsonl`);

const db = new DatabaseSync(dbPath);
const backup = db.prepare("SELECT value FROM kv WHERE key = 'settings'").get()?.value ?? null;
const settings = backup ? JSON.parse(backup) : {};
settings.provider = { ...(settings.provider ?? {}), preset: "ollama", baseUrl: `http://127.0.0.1:${port}`, model: "mock-model", visionModel: "", temperature: 0.3 };
settings.privacy = { ...(settings.privacy ?? {}), storeHistory: true };
db.prepare("INSERT INTO kv(key, value) VALUES('settings', ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value").run(JSON.stringify(settings));

const server = spawn(process.execPath, [path.join(import.meta.dirname, "mock-llm.mjs"), String(port), logPath], { stdio: "ignore" });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function ask(text) {
  const started = new Date().toISOString();
  execFileSync(exe, ["--ask", text, "--quiet"]);
  for (let i = 0; i < 100; i++) {
    await sleep(200);
    const task = db.prepare("SELECT * FROM tasks WHERE request = ? AND started_at >= ? ORDER BY started_at DESC LIMIT 1").get(text, started);
    if (task && task.status !== "running") {
      const msgs = db
        .prepare("SELECT payload, tool_title, tool_ok FROM messages WHERE conversation_id = ? AND created_at >= ? ORDER BY id")
        .all(task.conversation_id, started)
        .map((m) => ({ ...JSON.parse(m.payload), toolTitle: m.tool_title, toolOk: m.tool_ok }));
      return { task, msgs, ms: Date.parse(task.finished_at) - Date.parse(task.started_at) };
    }
  }
  throw new Error(`timeout: ${text}`);
}

const text = (m) => (m.parts ?? []).filter((p) => p.type === "text").map((p) => p.text).join("");
const results = [];
function check(name, cond, detail) {
  results.push({ name, ok: !!cond, detail });
}

try {
  await sleep(600);

  // 1. Simple request → one native tool → the model reports from the verified result.
  let r = await ask("какая сейчас громкость");
  let tools = r.msgs.filter((m) => m.role === "tool");
  let final = r.msgs.filter((m) => m.role === "assistant").pop();
  check("single tool: status", r.task.status === "completed", r.task.status);
  check("single tool: ran and verified", tools.length === 1 && tools[0].toolOk === 1, tools.map((t) => t.toolTitle));
  // The summary depends on the machine: "Громкость N%" or "Звук выключен" when muted.
  const volumeSummary = /^Проверил: (Громкость|Звук выключен)/;
  check("single tool: answer from verified result", volumeSummary.test(text(final)), text(final));
  check("single tool: two model calls", r.task.steps === 2, r.task.steps);

  // 2. Parallel independent tools in one step.
  r = await ask("покажи громкость и яркость");
  tools = r.msgs.filter((m) => m.role === "tool");
  final = r.msgs.filter((m) => m.role === "assistant").pop();
  check("parallel: two tools", tools.length === 2 && tools.every((t) => t.toolOk === 1), tools.map((t) => `${t.toolTitle}:${t.toolOk}`));
  check("parallel: combined answer", /Громкость|Звук выключен/.test(text(final)) && text(final).includes("Яркость"), text(final));

  // 3. Non-fast tool → second model round with the tool result.
  r = await ask("осмотрись");
  final = r.msgs.filter((m) => m.role === "assistant").pop();
  check("observe: second step", r.task.steps === 2, r.task.steps);
  check("observe: model saw verified result", text(final).startsWith("Проверил: Активное окно"), text(final));

  // 4. Recovery path: unknown tool → error returned to the model, turn still completes.
  r = await ask("вызови сломанный инструмент");
  tools = r.msgs.filter((m) => m.role === "tool");
  final = r.msgs.filter((m) => m.role === "assistant").pop();
  check("recover: tool error captured", tools.length === 1 && tools[0].toolOk === 0, text(tools[0] ?? {}));
  check("recover: model informed", text(final).includes("ошибка"), text(final));

  // Request shape sent to the provider.
  const reqs = fs.readFileSync(logPath, "utf8").trim().split("\n").map((l) => JSON.parse(l));
  const first = reqs[0].body;
  check("request: streaming + tools", first.stream === true && Array.isArray(first.tools) && first.tools.length >= 20, `${first.tools?.length} tools`);
  check("request: system prompt with context", first.messages[0].role === "system" && first.messages[0].content.includes("Foreground window"), "");
  check("request: no auth header for keyless preset", reqs.every((q) => q.auth === null), "");
} catch (e) {
  results.push({ name: "harness", ok: false, detail: String(e) });
} finally {
  if (backup === null) db.prepare("DELETE FROM kv WHERE key = 'settings'").run();
  else db.prepare("UPDATE kv SET value = ? WHERE key = 'settings'").run(backup);
  server.kill();
}

for (const r of results) console.log(`${r.ok ? "PASS" : "FAIL"}  ${r.name}${r.ok ? "" : `  →  ${JSON.stringify(r.detail)}`}`);
const failed = results.filter((r) => !r.ok).length;
console.log(`\n${results.length - failed}/${results.length} passed`);
process.exit(failed ? 1 : 0);
