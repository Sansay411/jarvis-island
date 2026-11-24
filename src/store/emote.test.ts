import { describe, expect, it } from "vitest";
import { ACTIVITIES, ACTIVITY_HOLD_MS, activityOf, activityStep, type Activity } from "./emote";
import toolNames from "./toolNames.json";

/** Every built-in tool registered in `src-tauri/src/tools/*`; a Rust test
 *  (`tools::tests`) keeps this file equal to the registry. */
const REGISTERED_TOOLS: string[] = toolNames;

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

describe("activityStep", () => {
  it("starts the first activity at once", () => {
    expect(activityStep(null, 0, "search", 10)).toEqual({ show: "search", wait: 0 });
  });

  it("keeps the shown activity until it has played the hold time", () => {
    expect(activityStep("search", 1000, "read", 1200)).toEqual({ show: "search", wait: ACTIVITY_HOLD_MS - 200 });
    expect(activityStep("search", 1000, "read", 1000 + ACTIVITY_HOLD_MS)).toEqual({ show: "read", wait: 0 });
  });

  it("holds the last activity a moment after the tool finished", () => {
    expect(activityStep("type", 1000, null, 1100)).toEqual({ show: "type", wait: ACTIVITY_HOLD_MS - 100 });
    expect(activityStep("type", 1000, null, 5000)).toEqual({ show: null, wait: 0 });
  });

  it("does nothing when the activity is unchanged", () => {
    expect(activityStep("code", 1000, "code", 1001)).toEqual({ show: "code", wait: 0 });
  });
});
