// How the window is divided among the open tabs, as an immutable tree. A leaf is a group: tabs that
// share one area of the window, one of them shown. A split divides its area among its children,
// side by side or stacked. Every function here returns a new tree and keeps the invariants below, so
// callers never meet an empty group, a split with one child, or a split nested in one of its axis.

export type Side = "left" | "right" | "top" | "bottom";
/** Where a tab is dropped on a group: into it, or beside it in a new group. */
export type DropZone = Side | "center";
/** `row`: children side by side, left to right. `column`: stacked, top to bottom. */
export type Axis = "row" | "column";

export interface Group<T> {
  readonly kind: "group";
  /** The same for as long as the group exists, whatever its tabs. */
  readonly id: number;
  /** In tab-strip order; never empty. */
  readonly tabs: readonly T[];
  /** The tab on screen; one of `tabs`. */
  readonly shown: T;
}

export interface Split<T> {
  readonly kind: "split";
  /** The same for as long as the split exists, whatever its children. */
  readonly id: number;
  readonly axis: Axis;
  /** At least two, none of them a split of the same axis. */
  readonly children: readonly Layout<T>[];
  /** Each child's share of the split; they add up to 1. */
  readonly sizes: readonly number[];
}

export type Layout<T> = Group<T> | Split<T>;

/** Where a tab goes: among group `group`'s tabs at `index` (the end by default), or beside it. */
export interface Target {
  group: number;
  zone: DropZone;
  /** For `center`: the position among the group's other tabs. */
  index?: number;
}

/** An area, in whatever unit the caller measures in. */
export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** The boundary after child `index` of split `split`, which the user drags to resize its neighbors. */
export interface Sash {
  split: number;
  index: number;
  axis: Axis;
  /** The split's area. */
  rect: Rect;
  /** Where along the split the boundary is, as a share of it from its start. */
  at: number;
}

export interface Arrangement<T> {
  /** In reading order: left before right, top before bottom. */
  groups: { group: Group<T>; rect: Rect }[];
  sashes: Sash[];
}

/** How a layout is saved: tabs as indexes into the saved list of tabs. */
export type SavedLayout =
  | { tabs: number[]; shown: number }
  | { axis: Axis; sizes: number[]; children: SavedLayout[] };

const WHOLE: Rect = { x: 0, y: 0, width: 1, height: 1 };
/** How far in from a side, as a share of the group's area, a drop still goes beside the group. */
const EDGE_SHARE = 0.3;

let lastId = 0;
const newId = (): number => ++lastId;

const newGroup = <T>(tab: T): Group<T> => ({ kind: "group", id: newId(), tabs: [tab], shown: tab });

/**
 * A split of `children` along `axis`, as the invariants allow: missing children are left out, their
 * room going to the others in proportion; splits of the same axis are merged in; and what is left of
 * a split with one child is that child.
 */
function split<T>(
  axis: Axis,
  children: readonly (Layout<T> | null)[],
  sizes: readonly number[],
  id = newId(),
): Layout<T> | null {
  const kept: Layout<T>[] = [];
  const shares: number[] = [];
  children.forEach((child, index) => {
    if (!child) return;
    if (child.kind === "split" && child.axis === axis) {
      kept.push(...child.children);
      shares.push(...child.sizes.map((size) => size * sizes[index]));
    } else {
      kept.push(child);
      shares.push(sizes[index]);
    }
  });
  if (kept.length <= 1) return kept[0] ?? null;
  const total = shares.reduce((sum, share) => sum + share, 0);
  return { kind: "split", id, axis, children: kept, sizes: shares.map((share) => share / total) };
}

/** The tree with each group replaced by what `change` makes of it; `null` removes the group. */
function rebuild<T>(layout: Layout<T>, change: (group: Group<T>) => Layout<T> | null): Layout<T> | null {
  if (layout.kind === "group") return change(layout);
  const children = layout.children.map((child) => rebuild(child, change));
  return split(layout.axis, children, layout.sizes, layout.id);
}

/**
 * `group` without `tab`, or `null` if it was its only tab. If the tab was the one shown, the tab to
 * its right is shown instead, or the one to its left if it was the last.
 */
function without<T>(group: Group<T>, tab: T): Group<T> | null {
  const index = group.tabs.indexOf(tab);
  if (index < 0) return group;
  const tabs = group.tabs.filter((other) => other !== tab);
  if (tabs.length === 0) return null;
  return { ...group, tabs, shown: group.shown === tab ? tabs[Math.min(index, tabs.length - 1)] : group.shown };
}

function inserted<T>(tabs: readonly T[], tab: T, index = tabs.length): T[] {
  const at = Math.min(Math.max(index, 0), tabs.length);
  return [...tabs.slice(0, at), tab, ...tabs.slice(at)];
}

/** The groups in reading order: left before right, top before bottom. */
export function groups<T>(layout: Layout<T> | null): Group<T>[] {
  if (!layout) return [];
  return layout.kind === "group" ? [layout] : layout.children.flatMap(groups);
}

/** The group `tab` is in, if it is in the layout. */
export function groupOf<T>(layout: Layout<T> | null, tab: T): Group<T> | null {
  return groups(layout).find((group) => group.tabs.includes(tab)) ?? null;
}

/**
 * Adds `tab`, which the layout doesn't have yet, to group `at.group` at `at.index` of its tabs (the
 * end by default), without showing it. Without that group it joins the first one; without a layout
 * it becomes the only group.
 */
export function addTab<T>(layout: Layout<T> | null, tab: T, at?: { group: number; index?: number }): Layout<T> {
  const all = groups(layout);
  if (!layout || all.length === 0) return newGroup(tab);
  const into = all.find((group) => group.id === at?.group) ?? all[0];
  const add = (group: Group<T>) => (group === into ? { ...group, tabs: inserted(group.tabs, tab, at?.index) } : group);
  return rebuild(layout, add) ?? layout;
}

/** Shows `tab` in its group. */
export function show<T>(layout: Layout<T>, tab: T): Layout<T> {
  return rebuild(layout, (group) => (group.tabs.includes(tab) ? { ...group, shown: tab } : group)) ?? layout;
}

/**
 * Removes `tab`. Its group shows the tab that was to its right instead, or the one to its left. A
 * group left without tabs goes away, its neighbors taking its room; `null` when it was the last.
 */
export function removeTab<T>(layout: Layout<T>, tab: T): Layout<T> | null {
  return rebuild(layout, (group) => without(group, tab));
}

/**
 * Moves `tab` to `target` and shows it there: among the tabs of the target group, or alone in a new
 * group taking half of the target group's area on that side. The group it leaves is treated as by
 * `removeTab`. Nothing changes if the tab or the target group isn't in the layout, or if the tab
 * would go beside a group that has no other tab.
 */
export function moveTab<T>(layout: Layout<T>, tab: T, target: Target): Layout<T> {
  const from = groupOf(layout, tab);
  const to = groups(layout).find((group) => group.id === target.group);
  if (!from || !to) return layout;
  const { zone } = target;
  if (zone !== "center" && from === to && from.tabs.length === 1) return layout;

  const move = (group: Group<T>): Layout<T> | null => {
    const rest = without(group, tab);
    if (group !== to) return rest;
    if (zone === "center") return { ...group, tabs: inserted(rest?.tabs ?? [], tab, target.index), shown: tab };
    const first = zone === "left" || zone === "top";
    const pair = first ? [newGroup(tab), rest] : [rest, newGroup(tab)];
    return split(zone === "left" || zone === "right" ? "row" : "column", pair, [0.5, 0.5]);
  };
  return rebuild(layout, move) ?? layout;
}

/**
 * Moves the boundary after child `index` of split `splitId` to `at`, a share of the split measured
 * from its start, leaving each of the two children it separates at least `min` of the split. The
 * other children keep their sizes. Nothing changes without such a boundary, or without room for
 * both minimums.
 */
export function resize<T>(layout: Layout<T>, splitId: number, index: number, at: number, min = 0): Layout<T> {
  if (layout.kind === "group") return layout;
  if (layout.id !== splitId) {
    return { ...layout, children: layout.children.map((child) => resize(child, splitId, index, at, min)) };
  }
  if (index < 0 || index + 1 >= layout.sizes.length) return layout;
  const sizes = [...layout.sizes];
  const start = sizes.slice(0, index).reduce((sum, size) => sum + size, 0);
  const end = start + sizes[index] + sizes[index + 1];
  if (end - start < 2 * min) return layout;
  const boundary = Math.min(Math.max(at, start + min), end - min);
  sizes[index] = boundary - start;
  sizes[index + 1] = end - boundary;
  return { ...layout, sizes };
}

/** Where each group is within `area` (the unit square by default), and the boundaries between them. */
export function arrange<T>(layout: Layout<T> | null, area: Rect = WHOLE): Arrangement<T> {
  const arrangement: Arrangement<T> = { groups: [], sashes: [] };
  if (layout) place(layout, area, arrangement);
  return arrangement;
}

function place<T>(layout: Layout<T>, area: Rect, into: Arrangement<T>): void {
  if (layout.kind === "group") {
    into.groups.push({ group: layout, rect: area });
    return;
  }
  let at = 0;
  layout.children.forEach((child, index) => {
    const size = layout.sizes[index];
    if (index > 0) into.sashes.push({ split: layout.id, index: index - 1, axis: layout.axis, rect: area, at });
    const rect =
      layout.axis === "row"
        ? { ...area, x: area.x + at * area.width, width: size * area.width }
        : { ...area, y: area.y + at * area.height, height: size * area.height };
    place(child, rect, into);
    at += size;
  });
}

/** The group in the window's top right corner. */
export function topRight<T>(layout: Layout<T>): Group<T> {
  if (layout.kind === "group") return layout;
  return topRight(layout.axis === "row" ? layout.children[layout.children.length - 1] : layout.children[0]);
}

/**
 * Where a tab dropped at (`x`, `y`) of a group's area goes, both as shares of the area from its top
 * left: beside the group when near one of its sides (the nearest), into it otherwise.
 */
export function dropZone(x: number, y: number): DropZone {
  const sides: [Side, number][] = [
    ["left", x],
    ["right", 1 - x],
    ["top", y],
    ["bottom", 1 - y],
  ];
  const [side, distance] = sides.reduce((nearest, next) => (next[1] < nearest[1] ? next : nearest));
  return distance < EDGE_SHARE ? side : "center";
}

/** The part of a group's `area` that a tab dropped in `zone` takes. */
export function zoneArea(zone: DropZone, area: Rect = WHOLE): Rect {
  const { x, y, width, height } = area;
  switch (zone) {
    case "left":
      return { x, y, width: width / 2, height };
    case "right":
      return { x: x + width / 2, y, width: width / 2, height };
    case "top":
      return { x, y, width, height: height / 2 };
    case "bottom":
      return { x, y: y + height / 2, width, height: height / 2 };
    case "center":
      return area;
  }
}

/** `layout` as it is saved, each tab as its index in `order`. */
export function saveLayout<T>(layout: Layout<T>, order: readonly T[]): SavedLayout {
  if (layout.kind === "group") {
    return { tabs: layout.tabs.map((tab) => order.indexOf(tab)), shown: order.indexOf(layout.shown) };
  }
  return {
    axis: layout.axis,
    sizes: [...layout.sizes],
    children: layout.children.map((child) => saveLayout(child, order)),
  };
}

/** Reads a saved layout leniently: anything that isn't shaped like one (hand-edited, a future format) is `null`. */
export function parseLayout(value: unknown): SavedLayout | null {
  if (typeof value !== "object" || value === null) return null;
  if ("tabs" in value) {
    const { tabs } = value;
    if (!Array.isArray(tabs) || !tabs.every((tab) => Number.isInteger(tab))) return null;
    const shown = "shown" in value && Number.isInteger(value.shown) ? (value.shown as number) : -1;
    return { tabs, shown };
  }
  if (!("axis" in value) || !("sizes" in value) || !("children" in value)) return null;
  const { axis, sizes, children } = value;
  if (axis !== "row" && axis !== "column") return null;
  if (!Array.isArray(sizes) || !Array.isArray(children) || sizes.length !== children.length) return null;
  if (!sizes.every((size) => typeof size === "number" && Number.isFinite(size) && size > 0)) return null;
  const parsed = children.map(parseLayout);
  if (!parsed.every((child) => child !== null)) return null;
  return { axis, sizes, children: parsed };
}

/**
 * The layout of `tabs` that `saved` describes, its indexes being positions in `tabs`. Entries that
 * are `null` (tabs that didn't come back) are left out, as are indexes without a tab and tabs
 * already placed. Tabs the saved layout doesn't place join the first group, which without a saved
 * layout is the only one. `null` when there is no tab at all.
 */
export function restoreLayout<T>(saved: SavedLayout | null, tabs: readonly (T | null)[]): Layout<T> | null {
  const placed = new Set<T>();
  const take = (index: number): T[] => {
    const tab = tabs[index] ?? null;
    if (tab === null || placed.has(tab)) return [];
    placed.add(tab);
    return [tab];
  };
  const build = (node: SavedLayout): Layout<T> | null => {
    if ("children" in node) return split(node.axis, node.children.map(build), node.sizes);
    const own = node.tabs.flatMap(take);
    if (own.length === 0) return null;
    const shown = tabs[node.shown] ?? null;
    return { kind: "group", id: newId(), tabs: own, shown: shown !== null && own.includes(shown) ? shown : own[0] };
  };

  let layout = saved ? build(saved) : null;
  for (const tab of tabs.flatMap((_, index) => take(index))) layout = addTab(layout, tab);
  return layout;
}
