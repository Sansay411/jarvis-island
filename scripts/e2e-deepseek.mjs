// Real provider + native tools + Search/Fetch. No keys in this script.
// Usage: node scripts/e2e-deepseek.mjs <path-to-jarvis.exe>
import { DatabaseSync } from "node:sqlite";
import { execFileSync } from "node:child_process";
import path from "node:path";

const exe = process.argv[2];
if (!exe) throw new Error("Pass the running Jarvis executable");
const db = new DatabaseSync(path.join(process.env.LOCALAPPDATA, "dev.jarvis.desktop", "jarvis.db"));
const config = JSON.parse(db.prepare("SELECT value FROM kv WHERE key='settings'").get().value);
if (config.provider.preset !== "deepseek" || config.provider.model !== "deepseek-flash") throw new Error("Configure official DeepSeek first");
const userConversation = db.prepare("SELECT conversation_id AS id FROM messages ORDER BY id DESC LIMIT 1").get()?.id;
const conversations = new Set();
const results = [];
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const text = (m) => (m.parts ?? []).filter((p) => p.type === "text").map((p) => p.text).join("");

try {
  for (const [name, prompt, expected] of [
    ["native", "Какая сейчас громкость?", "system_volume"],
    ["conversation", "Привет, Джарвис. Ответь одной короткой фразой, как настроение?", null],
    ["search", "Найди в интернете официальный анонс DeepSeek V4.1 Flash. Дай краткий ответ и ссылку на источник.", "web_search"],
    ["fetch", "Прочитай https://api-docs.deepseek.com/quick_start/pricing/ и скажи название актуальной версии Flash со ссылкой на страницу.", "web_fetch"],
  ]) {
    execFileSync(exe, ["--command", "new-conversation"]);
    const since = new Date().toISOString();
    execFileSync(exe, ["--ask", prompt, "--quiet"]);
    let task;
    for (let i = 0; i < 480; i++) {
      await sleep(250);
      task = db.prepare("SELECT * FROM tasks WHERE request=? AND started_at>=? ORDER BY started_at DESC LIMIT 1").get(prompt, since);
      if (task) conversations.add(task.conversation_id);
      if (task && task.status !== "running") break;
    }
    if (!task || task.status === "running") throw new Error(`Timeout: ${name}`);
    const messages = db.prepare("SELECT payload,tool_ok FROM messages WHERE conversation_id=? ORDER BY id").all(task.conversation_id).map((r) => ({...JSON.parse(r.payload),toolOk:r.tool_ok}));
    const calls = messages.flatMap((m) => m.tool_calls ?? []);
    const answer = text(messages.filter((m) => m.role === "assistant").at(-1) ?? {});
    const successful = messages.filter((m) => m.role === "tool" && m.toolOk === 1);
    const ok = task.status === "completed" && !!answer && (expected ? calls.some((c) => c.name === expected) && successful.length > 0 : calls.length === 0)
      && (name !== "native" || task.steps === 1)
      && (!['search','fetch'].includes(name) || /https:\/\//.test(answer));
    const result = {name,ok,status:task.status,ms:Date.parse(task.finished_at)-Date.parse(task.started_at),steps:task.steps,tools:calls.map((c) => c.name),answer:answer.slice(0,500),error:task.error};
    results.push(result);
    console.log(JSON.stringify(result));
  }
} finally {
  execFileSync(exe, ["--command", "cancel-task"]);
  await sleep(400);
  for (const id of conversations) {
    if (db.prepare("SELECT 1 FROM tasks WHERE conversation_id=? AND status='running'").get(id)) continue;
    db.prepare("DELETE FROM messages WHERE conversation_id=?").run(id);
    db.prepare("DELETE FROM tasks WHERE conversation_id=?").run(id);
    db.prepare("DELETE FROM conversations WHERE id=?").run(id);
  }
  execFileSync(exe, userConversation ? ["--conversation",userConversation] : ["--command","new-conversation"]);
  db.close();
}
if (results.length !== 4 || results.some((r) => !r.ok)) process.exitCode=1;
