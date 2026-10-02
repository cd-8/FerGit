import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Target } from "./layout";
import { TabDrag, type Drop } from "./tabDrag.svelte";
import type { Tab } from "./tabs.svelte";

const tab = { session: 1 } as Tab;
const RIGHT: Drop = { target: { group: 7, zone: "right" }, area: { x: 400, y: 0, width: 400, height: 600 } };

/** A pointer event of the stand-in window: only what `TabDrag` reads. */
function pointer(type: string, x: number, y: number, pointerId = 1): PointerEvent {
  return Object.assign(new Event(type), { pointerId, clientX: x, clientY: y }) as PointerEvent;
}

/** A drag whose tab can land (at `RIGHT`) only right of x = 400. */
function setup() {
  const dropped: Target[] = [];
  const drag = new TabDrag(
    (_, x) => (x > 400 ? RIGHT : null),
    (_, target) => dropped.push(target),
  );
  const send = (type: string, x = 0, y = 0, pointerId = 1) => window.dispatchEvent(pointer(type, x, y, pointerId));
  return { drag, dropped, send };
}

describe("TabDrag", () => {
  beforeEach(() => vi.stubGlobal("window", new EventTarget()));
  afterEach(() => vi.unstubAllGlobals());

  it("leaves a press that hardly moves a click", () => {
    const { drag, dropped, send } = setup();
    drag.press(tab, pointer("pointerdown", 100, 10));
    send("pointermove", 103, 12);
    expect(drag.current).toBeNull();
    send("pointerup", 103, 12);
    expect(dropped).toEqual([]);
  });

  it("follows the pointer once it has moved, and drops the tab where it can land", () => {
    const { drag, dropped, send } = setup();
    drag.press(tab, pointer("pointerdown", 100, 10));
    send("pointermove", 120, 10);
    expect(drag.current).toEqual({ tab, x: 120, y: 10, drop: null });
    // Back near where it started: still a drag.
    send("pointermove", 101, 10);
    expect(drag.current?.x).toBe(101);
    send("pointermove", 500, 300);
    expect(drag.current).toEqual({ tab, x: 500, y: 300, drop: RIGHT });

    send("pointerup", 500, 300);
    expect(drag.current).toBeNull();
    expect(dropped).toEqual([RIGHT.target]);
  });

  it("moves nothing when let go where the tab can't land", () => {
    const { drag, dropped, send } = setup();
    drag.press(tab, pointer("pointerdown", 100, 10));
    send("pointermove", 200, 300);
    send("pointerup", 200, 300);
    expect(drag.current).toBeNull();
    expect(dropped).toEqual([]);
  });

  it("stops when cancelled, or when the window loses the pointer or focus", () => {
    for (const stop of ["cancel", "pointercancel", "blur"]) {
      const { drag, dropped, send } = setup();
      drag.press(tab, pointer("pointerdown", 100, 10));
      send("pointermove", 500, 300);
      if (stop === "cancel") drag.cancel();
      else send(stop);
      expect(drag.current).toBeNull();
      send("pointermove", 600, 300);
      send("pointerup", 600, 300);
      expect(drag.current).toBeNull();
      expect(dropped).toEqual([]);
    }
  });

  it("ignores other pointers", () => {
    const { drag, dropped, send } = setup();
    drag.press(tab, pointer("pointerdown", 100, 10));
    send("pointermove", 500, 300, 2);
    expect(drag.current).toBeNull();
    send("pointermove", 500, 300);
    send("pointerup", 500, 300, 2);
    expect(drag.current?.drop).toEqual(RIGHT);
    expect(dropped).toEqual([]);
  });
});
