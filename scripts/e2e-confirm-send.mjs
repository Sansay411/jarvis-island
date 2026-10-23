// End-to-end check that Enter in a messenger is never pressed without the
// user's confirmation. No real messenger is involved: a copy of PowerShell
// named telegram.exe shows an empty form with a text box, which JARVIS's
// policy treats as a messenger window. The scripted model (mock-llm.mjs)
// asks for input_key "enter"; the test expects a high-risk confirmation in
// the log, cancels the task and checks the form never received Enter.
// Settings are backed up and restored.
//
//   node scripts/e2e-confirm-send.mjs <path-to-running-jarvis.exe> [port]

import { DatabaseSync } from "node:sqlite";
import { execFileSync, spawn } from "node:child_process";
import path from "node:path";
import fs from "node:fs";
import os from "node:os";

const exe = process.argv[2];
const port = Number(process.argv[3] ?? 18081);
const appDir = path.join(process.env.LOCALAPPDATA, "dev.jarvis.desktop");
const logFile = path.join(appDir, "logs", "jarvis.log");
const work = fs.mkdtempSync(path.join(os.tmpdir(), "jarvis-confirm-"));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const db = new DatabaseSync(path.join(appDir, "jarvis.db"));
const backup = db.prepare("SELECT value FROM kv WHERE key = 'settings'").get()?.value ?? null;
const settings = backup ? JSON.parse(backup) : {};
settings.provider = { ...(settings.provider ?? {}), preset: "ollama", baseUrl: `http://127.0.0.1:${port}`, model: "mock-model", visionModel: "", temperature: 0.3 };
settings.agent = { ...(settings.agent ?? {}), confirmModerate: true };
db.prepare("INSERT INTO kv(key, value) VALUES('settings', ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value").run(JSON.stringify(settings));

// Run in a conversation of its own and put the user back in theirs after:
// test turns must never become part of the user's chat or the model's context.
const testStart = new Date().toISOString();
const userConversation = db.prepare("SELECT conversation_id AS id FROM messages ORDER BY id DESC LIMIT 1").get()?.id ?? null;
execFileSync(exe, ["--command", "new-conversation"]);
function restoreConversation() {
  const ids = db
    .prepare("SELECT DISTINCT conversation_id AS id FROM messages WHERE created_at >= ?")
    .all(testStart)
    .map((r) => r.id)
    .filter((id) => id !== userConversation);
  for (const id of ids) {
    db.prepare("DELETE FROM messages WHERE conversation_id = ?").run(id);
    db.prepare("DELETE FROM tasks WHERE conversation_id = ?").run(id);
    db.prepare("DELETE FROM conversations WHERE id = ?").run(id);
  }
  execFileSync(exe, userConversation ? ["--conversation", userConversation] : ["--command", "new-conversation"]);
}

const server = spawn(process.execPath, [path.join(import.meta.dirname, "mock-llm.mjs"), String(port), path.join(work, "mock.jsonl")], { stdio: "ignore" });

// The fake messenger: a PowerShell copy named telegram.exe with a text box.
const fake = path.join(work, "telegram.exe");
fs.copyFileSync(path.join(process.env.SystemRoot, "System32", "WindowsPowerShell", "v1.0", "powershell.exe"), fake);
const marker = path.join(work, "enter-pressed");
const active = path.join(work, "form-active");
const form = `
Add-Type -AssemblyName System.Windows.Forms
$f = New-Object Windows.Forms.Form
$f.Text = 'Нинок (тестовый чат JARVIS)'
$f.TopMost = $true
$t = New-Object Windows.Forms.TextBox
$t.Text = 'тестовое сообщение'
$t.Dock = 'Fill'
$t.Add_KeyDown({ if ($_.KeyCode -eq 'Enter') { Set-Content -Path '${marker}' -Value 'enter' } })
$f.Controls.Add($t)
$f.Add_Shown({ $f.Activate(); $t.Focus() })
$probe = New-Object Windows.Forms.Timer
$probe.Interval = 200
$probe.Add_Tick({ if ([Windows.Forms.Form]::ActiveForm -eq $f) { Set-Content -Path '${active}' -Value 'active' } else { Remove-Item -Path '${active}' -ErrorAction SilentlyContinue } })
$probe.Start()
$timer = New-Object Windows.Forms.Timer
$timer.Interval = 40000
$timer.Add_Tick({ $f.Close() })
$timer.Start()
[void]$f.ShowDialog()
`;
const fakeProc = spawn(fake, ["-NoProfile", "-Command", form], { stdio: ["ignore", "pipe", "pipe"] });
let fakeOutput = "";
for (const stream of [fakeProc.stdout, fakeProc.stderr]) stream.on("data", (chunk) => { fakeOutput = (fakeOutput + chunk.toString()).slice(-3000); });

const results = [];
const check = (name, ok, detail = "") => results.push({ name, ok: !!ok, detail });

try {
  // Never send the test Enter anywhere but the fake window.
  for (let i = 0; i < 40 && !fs.existsSync(active); i++) await sleep(250);
  if (!fs.existsSync(active)) throw new Error(`the fake messenger window did not get the foreground — not asking (exit=${fakeProc.exitCode}; output=${fakeOutput})`);
  const logStart = fs.statSync(logFile).size;
  execFileSync(exe, ["--ask", "нажми enter", "--quiet"]);
  let line = null;
  for (let i = 0; i < 60 && !line; i++) {
    await sleep(250);
    const tail = fs.readFileSync(logFile).subarray(logStart).toString("utf8");
    line = tail.split("\n").find((l) => l.includes("confirmation requested") && l.includes("input_key"));
  }
  check("confirmation requested for Enter in a messenger", line, line ?? "no confirmation in the log");
  check("risk is high", /risk="?high/.test(line ?? ""), line ?? "");
  check("title says it sends a message", line?.includes("Отправить сообщение"), line ?? "");
  execFileSync(exe, ["--command", "cancel-task"]);
  await sleep(1500);
  check("Enter never reached the window", !fs.existsSync(marker));
} catch (e) {
  check("harness", false, String(e));
} finally {
  restoreConversation();
  fakeProc.kill();
  server.kill();
  if (backup !== null) db.prepare("UPDATE kv SET value = ? WHERE key = 'settings'").run(backup);
  await sleep(300);
  if (path.dirname(path.resolve(work)) !== path.resolve(os.tmpdir()) || !path.basename(work).startsWith("jarvis-confirm-")) throw new Error("Unsafe test cleanup path");
  fs.rmSync(work, { recursive: true, force: true });
}

for (const r of results) console.log(`${r.ok ? "PASS" : "FAIL"}  ${r.name}${r.ok ? "" : `  →  ${r.detail}`}`);
const passed = results.filter((r) => r.ok).length;
console.log(`\n${passed}/${results.length} passed`);
process.exit(passed === results.length ? 0 : 1);
