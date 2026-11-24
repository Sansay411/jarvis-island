import { describe, expect, it } from "vitest";
import { ACTIVITIES, activityOf, type Activity } from "./emote";

/** Complete list of built-in tools registered in `src-tauri/src/tools/*`.
 *  Synced by a Rust test in `src-tauri/src/tools/mod.rs`. */
const REGISTERED_TOOLS = [
  "app_launch",
  "app_volume",
  "apps_find",
  "bluetooth_battery",
  "browser_close_tab",
  "browser_read_page",
  "browser_switch_tab",
  "browser_tabs",
  "calendar_manage",
  "clipboard_history",
  "clipboard_read",
  "clipboard_write",
  "coding_agent_run",
  "context_snapshot",
  "dev_tools_status",
  "display_brightness",
  "file_action",
  "file_read",
  "files_search",
  "focus_session",
  "growth",
  "input_click",
  "input_key",
  "input_scroll",
  "input_type",
  "job_cancel",
  "job_status",
  "location",
  "media_control",
  "memory_forget",
  "memory_save",
  "mission_plan",
  "open_target",
  "projects_list",
  "reminder_cancel",
  "reminder_create",
  "reminders_list",
  "screen_capture",
  "selection_grab",
  "shell_run",
  "speak_aloud",
  "stopwatch",
  "system_diagnostics",
  "system_volume",
  "tasks_manage",
  "terminal_open",
  "ui_act",
  "ui_inspect",
  "vscode_open",
  "wait",
  "weather",
  "web_fetch",
  "web_search",
  "window_action",
  "window_layout",
  "windows_list",
] as const;

describe("activityOf", () => {
  it("maps every registered built-in tool to a dedicated non-generic activity", () => {
    const unmapped = REGISTERED_TOOLS.filter((name) => activityOf(name) === "work");
    expect(unmapped).toEqual([]);
  });

  it("maps planned overlay and guide tools to draw", () => {
    expect(activityOf("screen_point")).toBe("draw");
    expect(activityOf("guide_step")).toBe("draw");
    expect(activityOf("explain_card")).toBe("draw");
  });

  it("maps MCP tools to connect", () => {
    expect(activityOf("mcp__github__create_issue")).toBe("connect");
    expect(activityOf("mcp__filesystem__read_file")).toBe("connect");
    expect(activityOf("mcp")).toBe("connect");
  });

  it("maps representative tools across all categories accurately", () => {
    expect(activityOf("web_search")).toBe("search");
    expect(activityOf("file_read")).toBe("read");
    expect(activityOf("ui_inspect")).toBe("read");
    expect(activityOf("input_type")).toBe("type");
    expect(activityOf("input_click")).toBe("click");
    expect(activityOf("ui_act")).toBe("click");
    expect(activityOf("screen_capture")).toBe("look");
    expect(activityOf("files_search")).toBe("files");
    expect(activityOf("media_control")).toBe("music");
    expect(activityOf("calendar_manage")).toBe("calendar");
    expect(activityOf("memory_save")).toBe("memory");
    expect(activityOf("reminder_create")).toBe("time");
    expect(activityOf("system_volume")).toBe("system");
    expect(activityOf("shell_run")).toBe("code");
    expect(activityOf("coding_agent_run")).toBe("code");
    expect(activityOf("speak_aloud")).toBe("speak");
  });

  it("falls back to generic work for unknown tool names", () => {
    expect(activityOf("some_future_tool")).toBe("work");
    expect(activityOf("")).toBe("work");
  });

  it("lists unique labels for every activity kind", () => {
    const ids = new Set<Activity>(ACTIVITIES.map((a) => a.id));
    expect(ids.size).toBe(16);
    expect(ACTIVITIES.every((a) => a.label.trim().length > 0)).toBe(true);
  });
});
