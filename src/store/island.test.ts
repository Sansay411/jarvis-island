import { afterEach, describe, expect, it, vi } from "vitest";
import { endInlineInput, resolveMode, useIsland } from "./island";
import { ipc } from "../lib/ipc";

vi.mock("../lib/ipc", () => ({ ipc: { island: { endInput: vi.fn() } } }));

afterEach(() => {
  vi.unstubAllGlobals();
  vi.resetAllMocks();
});

describe("pinned island", () => {
  it("stays open without the pointer and only the pin closes it", () => {
    vi.stubGlobal("localStorage", { getItem: vi.fn(), setItem: vi.fn() });
    useIsland.setState({ pinned: false, hovered: false, inputFocused: false, dragOver: false, voiceOpen: false, workspaceOpen: false, clipsOpen: false });
    useIsland.getState().togglePinned();
    expect(resolveMode(useIsland.getState())).toBe("peek");
    // The pointer leaving changes nothing while pinned.
    useIsland.getState().set({ hovered: false });
    expect(resolveMode(useIsland.getState())).toBe("peek");
    // Voice and the workspace still take over, and give the pinned deck back.
    useIsland.getState().set({ voiceOpen: true });
    expect(resolveMode(useIsland.getState())).toBe("voice");
    useIsland.getState().set({ voiceOpen: false });
    expect(resolveMode(useIsland.getState())).toBe("peek");
    useIsland.getState().togglePinned();
    expect(resolveMode(useIsland.getState())).toBe("compact");
    expect(localStorage.setItem).toHaveBeenLastCalledWith("jarvis.islandPinned", "0");
  });
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

describe("area pick invited voice", () => {
  it("keeps workspace visible while voice is auto-listening for a picked area", () => {
    expect(
      resolveMode({
        hovered: false,
        dragOver: false,
        workspaceOpen: true,
        voiceOpen: true,
        clipsOpen: false,
        inputFocused: false,
        areaVoiceInvited: true,
      }),
    ).toBe("workspace");
    expect(
      resolveMode({
        hovered: false,
        dragOver: false,
        workspaceOpen: true,
        voiceOpen: true,
        clipsOpen: false,
        inputFocused: false,
        areaVoiceInvited: false,
      }),
    ).toBe("voice");
  });
});
