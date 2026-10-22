// Scripted OpenAI-compatible Chat Completions server for end-to-end tests of
// the agent loop without a cloud key. It streams SSE exactly like the real
// API (fragmented tool-call arguments, usage chunk, [DONE]) and decides what
// to do from the conversation: a user message triggers the scenario's tool
// calls; tool results are summarised in a final answer. Every request is
// appended to a JSONL log for assertions.
//
//   node scripts/mock-llm.mjs <port> <log.jsonl>

import http from "node:http";
import fs from "node:fs";

const port = Number(process.argv[2] ?? 18080);
const logPath = process.argv[3] ?? "mock-llm.jsonl";

/** user text → tool calls for the first step */
function plan(userText) {
  const t = userText.toLowerCase();
  if (t.includes("громкость и яркость")) {
    return [
      { name: "system_volume", args: { action: "get" } },
      { name: "display_brightness", args: {} },
    ];
  }
  if (t.includes("громкость")) return [{ name: "system_volume", args: { action: "get" } }];
  if (t.includes("осмотрись")) return [{ name: "context_snapshot", args: {} }];
  if (t.includes("сломанный")) return [{ name: "no_such_tool", args: {} }];
  if (t.includes("нажми enter")) return [{ name: "input_key", args: { keys: "enter" } }];
  return [];
}

function sse(res, chunks) {
  res.writeHead(200, { "content-type": "text/event-stream", connection: "close" });
  for (const c of chunks) res.write(`data: ${JSON.stringify(c)}\n\n`);
  res.write("data: [DONE]\n\n");
  res.end();
}

function textChunks(text) {
  const parts = text.match(/.{1,12}/gsu) ?? [text];
  return [
    ...parts.map((p) => ({ choices: [{ index: 0, delta: { content: p } }] })),
    { choices: [{ index: 0, delta: {}, finish_reason: "stop" }] },
    { choices: [], usage: { prompt_tokens: 100, completion_tokens: 20 } },
  ];
}

function toolChunks(calls) {
  const chunks = [];
  calls.forEach((c, index) => {
    const args = JSON.stringify(c.args);
    const half = Math.ceil(args.length / 2);
    chunks.push({ choices: [{ index: 0, delta: { tool_calls: [{ index, id: `call_${index}_${Date.now()}`, type: "function", function: { name: c.name, arguments: "" } }] } }] });
    // DashScope-style fragments with empty late ids
    chunks.push({ choices: [{ index: 0, delta: { tool_calls: [{ index, id: "", function: { arguments: args.slice(0, half) } }] } }] });
    chunks.push({ choices: [{ index: 0, delta: { tool_calls: [{ index, id: "", function: { arguments: args.slice(half) } }] } }] });
  });
  chunks.push({ choices: [{ index: 0, delta: {}, finish_reason: "tool_calls" }] });
  return chunks;
}

http
  .createServer((req, res) => {
    let body = "";
    req.on("data", (d) => (body += d));
    req.on("end", () => {
      let json = {};
      try {
        json = JSON.parse(body || "{}");
      } catch {}
      fs.appendFileSync(logPath, JSON.stringify({ url: req.url, auth: req.headers.authorization ?? null, body: json }) + "\n");
      const msgs = json.messages ?? [];
      const last = msgs[msgs.length - 1] ?? {};
      if (last.role === "user") {
        const text = typeof last.content === "string" ? last.content : (last.content ?? []).map((p) => p.text ?? "").join("");
        const calls = plan(text);
        return sse(res, calls.length ? toolChunks(calls) : textChunks("Привет! Чем помочь?"));
      }
      if (last.role === "tool") {
        // Summarise every tool result since the last assistant message.
        const results = [];
        for (let i = msgs.length - 1; i >= 0 && msgs[i].role === "tool"; i--) {
          try {
            const r = JSON.parse(msgs[i].content);
            results.unshift(r.ok ? r.summary : `ошибка: ${r.error ?? r.summary}`);
          } catch {
            results.unshift(String(msgs[i].content).slice(0, 80));
          }
        }
        return sse(res, textChunks(`Проверил: ${results.join("; ")}.`));
      }
      return sse(res, textChunks("…"));
    });
  })
  .listen(port, "127.0.0.1", () => console.log(`mock LLM on http://127.0.0.1:${port}`));
