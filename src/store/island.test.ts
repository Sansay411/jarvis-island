import { afterEach, describe, expect, it, vi } from "vitest";
import { endInlineInput, resolveMode, useIsland } from "./island";
import { ipc } from "../lib/ipc";

vi.mock("../lib/ipc", () => ({ ipc: { island: { endInput: vi.fn() } } }));

afterEach(() => {
  vi.unstubAllGlobals();
  vi.resetAllMocks();
});

describe("inline input Escape", () => {
  it("blurs before releasing native input and collapses", async () => {
    vi.stubGlobal("window", { clearTimeout: vi.fn() });
    useIsland.setState({ inputFocused: true, hovered: true });
    const blur = vi.fn();
    vi.mocked(ipc.island.endInput).mockImplementation(async () => {
      expect(blur).toHaveBeenCalledOnce();
      expect(resolveMode(useIsland.getState())).toBe("compact");
    });
    await endInlineInput(blur);
    expect(ipc.island.endInput).toHaveBeenCalledWith(true);
  });

  it("handles an already blurred input", async () => {
    vi.stubGlobal("window", { clearTimeout: vi.fn() });
    useIsland.setState({ inputFocused: false, hovered: false });
    await endInlineInput(() => {});
    expect(useIsland.getState().inputFocused).toBe(false);
  });

  it("still clears input state if native focus restoration fails", async () => {
    vi.stubGlobal("window", { clearTimeout: vi.fn() });
    useIsland.setState({ inputFocused: true, hovered: true });
    vi.mocked(ipc.island.endInput).mockRejectedValueOnce(new Error("native failure"));
    await expect(endInlineInput(() => {})).rejects.toThrow("native failure");
    expect(resolveMode(useIsland.getState())).toBe("compact");
  });
});
