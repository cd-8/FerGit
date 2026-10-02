import { describe, expect, it } from "vitest";
import {
  addTab,
  arrange,
  dropZone,
  groupOf,
  groups,
  moveTab,
  parseLayout,
  removeTab,
  resize,
  restoreLayout,
  saveLayout,
  show,
  topRight,
  zoneArea,
  type DropZone,
  type Layout,
  type SavedLayout,
  type Split,
} from "./layout";

type L = Layout<string>;

/** A layout in a line: groups as their tabs with the shown one in brackets, splits as `axis(a | b)`. */
function shape(layout: L | null): string {
  if (!layout) return "";
  if (layout.kind === "group") return layout.tabs.map((tab) => (tab === layout.shown ? `[${tab}]` : tab)).join(" ");
  return `${layout.axis}(${layout.children.map(shape).join(" | ")})`;
}

/** One group of `tabs`, the first shown. */
function group(...tabs: string[]): L {
  return tabs.reduce<L | null>((layout, tab) => addTab(layout, tab), null) as L;
}

const idOf = (layout: L, tab: string): number => groupOf(layout, tab)!.id;
const sizes = (layout: L): readonly number[] => (layout as Split<string>).sizes;
/** `tab` moved to `zone` of the group `beside` is in. */
const moved = (layout: L, tab: string, zone: DropZone, beside: string, index?: number): L =>
  moveTab(layout, tab, { group: idOf(layout, beside), zone, index });

describe("tabs in groups", () => {
  it("starts with one group, and adds tabs at its end without showing them", () => {
    expect(shape(group("a"))).toBe("[a]");
    expect(shape(group("a", "b", "c"))).toBe("[a] b c");
  });

  it("adds a tab to the chosen group at the chosen place, or to the first group", () => {
    const layout = moved(group("a", "b", "c"), "c", "right", "a");
    expect(shape(layout)).toBe("row([a] b | [c])");
    expect(shape(addTab(layout, "d", { group: idOf(layout, "c") }))).toBe("row([a] b | [c] d)");
    expect(shape(addTab(layout, "d", { group: idOf(layout, "c"), index: 0 }))).toBe("row([a] b | d [c])");
    expect(shape(addTab(layout, "d", { group: -1 }))).toBe("row([a] b d | [c])");
    expect(shape(addTab(layout, "d"))).toBe("row([a] b d | [c])");
  });

  it("shows a tab in its own group only", () => {
    const layout = moved(group("a", "b", "c"), "c", "right", "a");
    expect(shape(show(layout, "b"))).toBe("row(a [b] | [c])");
    expect(shape(show(layout, "nowhere"))).toBe("row([a] b | [c])");
  });

  it("finds the groups in reading order, and the group of a tab", () => {
    const layout = moved(moved(group("a", "b", "c"), "b", "right", "a"), "c", "bottom", "b");
    expect(shape(layout)).toBe("row([a] | column([b] | [c]))");
    expect(groups(layout).map((each) => each.shown)).toEqual(["a", "b", "c"]);
    expect(groupOf(layout, "c")?.tabs).toEqual(["c"]);
    expect(groupOf(layout, "nowhere")).toBeNull();
    expect(groups(null)).toEqual([]);
  });
});

describe("removing a tab", () => {
  it("shows the tab to the right of a removed shown tab, or the one to its left", () => {
    const layout = show(group("a", "b", "c"), "b");
    expect(shape(removeTab(layout, "b"))).toBe("a [c]");
    expect(shape(removeTab(show(layout, "c"), "c"))).toBe("a [b]");
    expect(shape(removeTab(layout, "a"))).toBe("[b] c");
  });

  it("removes a group left without tabs, its neighbors taking its room in proportion", () => {
    let layout = moved(moved(group("a", "b", "c"), "b", "right", "a"), "c", "right", "b");
    expect(shape(layout)).toBe("row([a] | [b] | [c])");
    expect(sizes(layout)).toEqual([0.5, 0.25, 0.25]);

    layout = removeTab(layout, "c") as L;
    expect(shape(layout)).toBe("row([a] | [b])");
    expect(sizes(layout)[0]).toBeCloseTo(2 / 3);
    expect(shape(removeTab(layout, "a"))).toBe("[b]");
  });

  it("replaces a split left with one child by that child, merging it into a parent of its axis", () => {
    const layout = moved(moved(moved(group("a", "b", "c", "d"), "b", "right", "a"), "c", "bottom", "b"), "d", "right", "c");
    expect(shape(layout)).toBe("row([a] | column([b] | row([c] | [d])))");
    const merged = removeTab(layout, "b") as L;
    expect(shape(merged)).toBe("row([a] | [c] | [d])");
    expect(sizes(merged)).toEqual([0.5, 0.25, 0.25]);
  });

  it("leaves nothing when the last tab goes, and ignores tabs it doesn't have", () => {
    expect(removeTab(group("a"), "a")).toBeNull();
    expect(shape(removeTab(group("a"), "b"))).toBe("[a]");
  });
});

describe("moving a tab", () => {
  it("splits the target group in half on the side the tab is dropped", () => {
    const layout = group("a", "b");
    expect(shape(moved(layout, "b", "right", "a"))).toBe("row([a] | [b])");
    expect(shape(moved(layout, "b", "left", "a"))).toBe("row([b] | [a])");
    expect(shape(moved(layout, "b", "bottom", "a"))).toBe("column([a] | [b])");
    expect(shape(moved(layout, "b", "top", "a"))).toBe("column([b] | [a])");
    expect(sizes(moved(layout, "b", "top", "a"))).toEqual([0.5, 0.5]);
  });

  it("shows the next tab in the group a shown tab leaves", () => {
    expect(shape(moved(group("a", "b", "c"), "a", "right", "a"))).toBe("row([b] c | [a])");
  });

  it("puts a group beside its target in a split of the same axis, and nests one of the other", () => {
    const row = moved(group("a", "b", "c"), "b", "right", "a");
    const three = moved(row, "c", "left", "b");
    expect(shape(three)).toBe("row([a] | [c] | [b])");
    expect(sizes(three)).toEqual([0.5, 0.25, 0.25]);
    expect(shape(moved(row, "c", "top", "b"))).toBe("row([a] | column([c] | [b]))");
  });

  it("moves a tab into another group, shown, at the end or where asked", () => {
    const layout = moved(group("a", "b", "c", "d"), "d", "right", "a");
    expect(shape(moved(layout, "b", "center", "d"))).toBe("row([a] c | d [b])");
    expect(shape(moved(layout, "b", "center", "d", 0))).toBe("row([a] c | [b] d)");
  });

  it("removes the group a tab leaves empty", () => {
    const layout = moved(group("a", "b"), "b", "bottom", "a");
    expect(shape(moved(layout, "b", "center", "a"))).toBe("a [b]");
    expect(shape(moved(layout, "b", "left", "a"))).toBe("row([b] | [a])");
  });

  it("reorders the tabs of a group, counting places among the other tabs", () => {
    const layout = group("a", "b", "c");
    expect(shape(moved(layout, "a", "center", "a", 2))).toBe("b c [a]");
    expect(shape(moved(layout, "c", "center", "a", 0))).toBe("[c] a b");
    expect(shape(moved(layout, "b", "center", "a", 1))).toBe("a [b] c");
    expect(shape(moved(layout, "b", "center", "a", 99))).toBe("a c [b]");
  });

  it("keeps the ids of the groups that stay", () => {
    const layout = moved(group("a", "b", "c"), "c", "right", "a");
    const [left, right] = [idOf(layout, "a"), idOf(layout, "c")];
    const after = moved(layout, "b", "center", "c");
    expect([idOf(after, "a"), idOf(after, "c")]).toEqual([left, right]);
    expect(idOf(moved(layout, "b", "bottom", "c"), "b")).not.toBe(right);
  });

  it("changes nothing when the only tab of a group goes beside it, or without the tab or the group", () => {
    const layout = moved(group("a", "b"), "b", "right", "a");
    expect(moved(layout, "b", "left", "b")).toBe(layout);
    expect(moveTab(layout, "b", { group: -1, zone: "center" })).toBe(layout);
    expect(moveTab(layout, "nowhere", { group: idOf(layout, "a"), zone: "right" })).toBe(layout);
  });
});

describe("resizing", () => {
  const three = (): Split<string> =>
    moved(moved(group("a", "b", "c"), "b", "right", "a"), "c", "right", "b") as Split<string>;

  it("moves a boundary, changing only the two children it separates", () => {
    const layout = three();
    expect(sizes(resize(layout, layout.id, 0, 0.3))).toEqual([0.3, 0.45, 0.25]);
    expect(sizes(resize(layout, layout.id, 1, 0.9)).map((size) => +size.toFixed(2))).toEqual([0.5, 0.4, 0.1]);
  });

  it("keeps both children at least the minimum", () => {
    const layout = three();
    expect(sizes(resize(layout, layout.id, 0, 0.01, 0.2))).toEqual([0.2, 0.55, 0.25]);
    expect(sizes(resize(layout, layout.id, 0, 2, 0.2))[1]).toBeCloseTo(0.2);
    expect(resize(layout, layout.id, 1, 0.6, 0.3)).toBe(layout);
  });

  it("finds a nested split, and ignores boundaries that don't exist", () => {
    const layout = moved(three(), "c", "bottom", "b") as Split<string>;
    const column = layout.children[1] as Split<string>;
    const resized = resize(layout, column.id, 0, 0.25) as Split<string>;
    expect(sizes(resized.children[1])).toEqual([0.25, 0.75]);
    expect(sizes(resized)).toEqual(sizes(layout));
    expect(sizes(resize(layout, layout.id, 1, 0.2))).toEqual(sizes(layout));
    expect(sizes(resize(layout, -1, 0, 0.2))).toEqual(sizes(layout));
  });
});

describe("arranging", () => {
  // a on the left half; on the right, b over c.
  const layout = moved(moved(group("a", "b", "c"), "b", "right", "a"), "c", "bottom", "b") as Split<string>;

  it("gives each group its area, in reading order", () => {
    const placed = arrange(layout).groups.map(({ group: each, rect }) => [each.shown, rect]);
    expect(placed).toEqual([
      ["a", { x: 0, y: 0, width: 0.5, height: 1 }],
      ["b", { x: 0.5, y: 0, width: 0.5, height: 0.5 }],
      ["c", { x: 0.5, y: 0.5, width: 0.5, height: 0.5 }],
    ]);
    expect(arrange(layout, { x: 0, y: 0, width: 800, height: 600 }).groups[2].rect).toEqual({
      x: 400,
      y: 300,
      width: 400,
      height: 300,
    });
    expect(arrange(null)).toEqual({ groups: [], sashes: [] });
  });

  it("gives a boundary between each two neighbors, with the area of its split", () => {
    const column = layout.children[1] as Split<string>;
    expect(arrange(layout).sashes).toEqual([
      { split: layout.id, index: 0, axis: "row", rect: { x: 0, y: 0, width: 1, height: 1 }, at: 0.5 },
      { split: column.id, index: 0, axis: "column", rect: { x: 0.5, y: 0, width: 0.5, height: 1 }, at: 0.5 },
    ]);
  });

  it("finds the group in the top right corner", () => {
    expect(topRight(layout).shown).toBe("b");
    expect(topRight(moved(layout, "a", "bottom", "c")).shown).toBe("b");
    expect(topRight(moved(group("a", "b"), "b", "left", "a")).shown).toBe("a");
    expect(topRight(group("a")).shown).toBe("a");
  });
});

describe("dropping", () => {
  it("goes beside a group near a side, the nearest one, and into it otherwise", () => {
    expect(dropZone(0.5, 0.5)).toBe("center");
    expect(dropZone(0.1, 0.5)).toBe("left");
    expect(dropZone(0.9, 0.5)).toBe("right");
    expect(dropZone(0.5, 0.1)).toBe("top");
    expect(dropZone(0.5, 0.95)).toBe("bottom");
    expect(dropZone(0.1, 0.05)).toBe("top");
    expect(dropZone(0.35, 0.65)).toBe("center");
  });

  it("marks the half of the area a new group would take, or all of it", () => {
    const area = { x: 100, y: 50, width: 400, height: 200 };
    expect(zoneArea("left", area)).toEqual({ x: 100, y: 50, width: 200, height: 200 });
    expect(zoneArea("right", area)).toEqual({ x: 300, y: 50, width: 200, height: 200 });
    expect(zoneArea("top", area)).toEqual({ x: 100, y: 50, width: 400, height: 100 });
    expect(zoneArea("bottom", area)).toEqual({ x: 100, y: 150, width: 400, height: 100 });
    expect(zoneArea("center", area)).toEqual(area);
  });
});

describe("saving and restoring", () => {
  const tabs = ["a", "b", "c", "d"];
  const saved: SavedLayout = {
    axis: "row",
    sizes: [0.6, 0.4],
    children: [
      { tabs: [0, 1], shown: 1 },
      { axis: "column", sizes: [0.5, 0.5], children: [{ tabs: [2], shown: 2 }, { tabs: [3], shown: 3 }] },
    ],
  };

  it("restores what it saved", () => {
    const layout = restoreLayout(saved, tabs) as L;
    expect(shape(layout)).toBe("row(a [b] | column([c] | [d]))");
    expect(sizes(layout)).toEqual([0.6, 0.4]);
    expect(saveLayout(layout, tabs)).toEqual(saved);
    expect(parseLayout(JSON.parse(JSON.stringify(saved)))).toEqual(saved);
  });

  it("leaves out tabs that didn't come back, and the groups and splits left without any", () => {
    expect(shape(restoreLayout(saved, ["a", null, "c", "d"]))).toBe("row([a] | column([c] | [d]))");
    expect(shape(restoreLayout(saved, ["a", "b", null, "d"]))).toBe("row(a [b] | [d])");
    expect(shape(restoreLayout(saved, [null, null, null, "d"]))).toBe("[d]");
    expect(restoreLayout(saved, [null, null, null, null])).toBeNull();
    expect(restoreLayout(null, [])).toBeNull();
  });

  it("places each tab once, and tabs the saved layout leaves out in the first group", () => {
    expect(shape(restoreLayout(saved, ["a", "b", "a", "d", "e"]))).toBe("row(a [b] e | [d])");
    const odd: SavedLayout = { axis: "row", sizes: [1, 3], children: [{ tabs: [1, 9, 1], shown: 7 }, { tabs: [0], shown: 0 }] };
    const layout = restoreLayout(odd, ["a", "b", "c"]) as L;
    expect(shape(layout)).toBe("row([b] c | [a])");
    expect(sizes(layout)).toEqual([0.25, 0.75]);
  });

  it("puts every tab in one group without a saved layout", () => {
    expect(shape(restoreLayout(null, ["a", null, "b", "a"]))).toBe("[a] b");
  });

  it("reads anything not shaped like a layout as none", () => {
    const bad: unknown[] = [
      null,
      "row",
      [],
      {},
      { tabs: "0" },
      { tabs: [0.5], shown: 0 },
      { axis: "diagonal", sizes: [1, 1], children: [{ tabs: [0] }, { tabs: [1] }] },
      { axis: "row", sizes: [1], children: [{ tabs: [0] }, { tabs: [1] }] },
      { axis: "row", sizes: [1, 0], children: [{ tabs: [0] }, { tabs: [1] }] },
      { axis: "row", sizes: [1, "1"], children: [{ tabs: [0] }, { tabs: [1] }] },
      { axis: "row", sizes: [1, 1], children: [{ tabs: [0] }, { oops: true }] },
    ];
    for (const value of bad) expect(parseLayout(value)).toBeNull();
    expect(parseLayout({ tabs: [0, 1] })).toEqual({ tabs: [0, 1], shown: -1 });
  });
});
