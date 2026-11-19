import { describe, expect, it } from "vitest";
import { beginDragOut, dropGoesToSend, endDragOut, isOwnDrag, rowCapturesWheel } from "./dragGuard";

describe("drag-out guard", () => {
  it("treats drops of the files we are dragging out as our own", () => {
    beginDragOut(["C:\a.png"]);
    expect(isOwnDrag(["C:\a.png"])).toBe(true);
    expect(isOwnDrag([])).toBe(true); // over/leave events carry no paths while our drag runs
    endDragOut();
    expect(isOwnDrag(["C:\a.png"])).toBe(false);
    expect(isOwnDrag([])).toBe(false);
  });
});

describe("dropGoesToSend", () => {
  it("trusts the exact drop point over the last over-event flag", () => {
    expect(dropGoesToSend(false, true)).toBe(false);
    expect(dropGoesToSend(true, false)).toBe(true);
    expect(dropGoesToSend(null, true)).toBe(true);
    expect(dropGoesToSend(null, false)).toBe(false);
  });
});

describe("rowCapturesWheel", () => {
  it("captures only when the shelf row can scroll", () => {
    expect(rowCapturesWheel(600, 400)).toBe(true);
    expect(rowCapturesWheel(400, 400)).toBe(false);
  });
});
