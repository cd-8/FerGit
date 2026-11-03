import type { Filter, RepoInfo, SessionId } from "./bindings";
import type { RepoClient } from "./client";
import { Finder } from "./find.svelte";
import { Inspector } from "./inspector.svelte";
import {
  addTab,
  groupOf,
  groups,
  moveTab,
  removeTab,
  resize,
  restoreLayout,
  saveLayout,
  show,
  type Layout,
  type Target,
} from "./layout";
import { Operations } from "./operations.svelte";
import { Review } from "./review.svelte";
import type { SavedTabs } from "./settings.svelte";
import { RepoView } from "./view.svelte";

/** What `Tabs` needs from the backend. */
export interface TabsBackend {
  /** Opens the repository containing `path`, or finds the session that already has it open. */
  open(path: string): Promise<{ session: SessionId; info: RepoInfo }>;
  /** The commands of session `session`. */
  client(session: SessionId): RepoClient;
}

/** Where the open tabs are remembered across launches. */
export interface TabsMemory {
  tabs: SavedTabs;
}

/**
 * One open repository: its rows, scroll position and selection (the view), what is shown about the
 * selection (the inspector), the operations running on it, its find bar and filter, its pull
 * requests, and whether the details panel is open. All of it stays as it is while other tabs are in
 * front.
 */
export class Tab {
  readonly session: SessionId;
  readonly client: RepoClient;
  readonly view: RepoView;
  readonly inspector: Inspector;
  readonly operations: Operations;
  readonly finder: Finder;
  readonly review: Review;
  #info = $state.raw<RepoInfo>() as RepoInfo;
  /**
   * Counts refresh results and change events taken in, including those that leave the generation
   * unchanged. Views of the index and worktree, which snapshots don't cover, reload when it moves.
   */
  updates = $state(0);
  /** True while a refresh is on its way. */
  refreshing = $state(false);
  detailsOpen = $state(true);
  /** Whether the filter picker is open. */
  filterOpen = $state(false);

  constructor(session: SessionId, info: RepoInfo, client: RepoClient) {
    this.session = session;
    this.client = client;
    this.#info = info;
    this.view = new RepoView(info, client);
    this.inspector = new Inspector(client);
    this.operations = new Operations(client, (info) => this.adopt(info));
    this.finder = new Finder(() => this.view, client);
    this.review = new Review(client, this.operations);
    void this.review.findHost();
  }

  /** As of the newest open, refresh or change event. */
  get info(): RepoInfo {
    return this.#info;
  }

  /** Re-reads the repository; the view follows if anything visible changed. */
  async refresh(): Promise<void> {
    if (this.refreshing) return;
    this.refreshing = true;
    try {
      this.adopt(await this.client.refresh());
    } finally {
      this.refreshing = false;
    }
  }

  /**
   * Shows only the commits `filter` selects (no refs and no path: all of them). The rows change
   * like after a refresh, except that the selected commit, if still shown, stays in view.
   */
  async setFilter(filter: Filter): Promise<void> {
    this.view.followSelectionOnNextChange();
    const info = await this.client.setFilter(filter);
    // Nothing changed (the same filter): don't leave the next refresh following the selection.
    if (info.generation === this.#info.generation) this.view.followSelectionOnNextChange(false);
    this.adopt(info);
  }

  /** Takes in a refresh result or change event; one older than what is shown is ignored. */
  adopt(info: RepoInfo): void {
    if (info.generation < this.#info.generation) return;
    this.#info = info;
    this.updates++;
    this.view.adopt(info);
  }

  /** Stops the inspector's timers and closes the backend session. */
  close(): Promise<void> {
    this.inspector.dispose();
    this.finder.hide();
    return this.client.close();
  }
}

/**
 * The open tabs, how the window is divided among them, and which one the keyboard works on.
 *
 * Tabs are in groups, each showing one of its tabs. The window starts as one group, and is split by
 * moving a tab beside a group (`layout.ts` has the rules). The active tab is the one shown in the
 * group the user is working in.
 *
 * A repository is open in at most one tab: opening it again (by any path inside it) activates its
 * tab. The backend guarantees one session per repository, so tabs are told apart by session. Every
 * change to the tabs, to the layout or to the active tab is remembered, so the next launch can
 * `restore` them. Reads are reactive.
 */
export class Tabs {
  readonly #backend: TabsBackend;
  readonly #memory: TabsMemory;
  #layout = $state.raw<Layout<Tab> | null>(null);
  #active = $state.raw<Tab | null>(null);
  #restoring = $state(false);

  constructor(backend: TabsBackend, memory: TabsMemory) {
    this.#backend = backend;
    this.#memory = memory;
  }

  /** Every open tab, group by group in reading order. */
  get all(): readonly Tab[] {
    return groups(this.#layout).flatMap((group) => group.tabs);
  }

  /** The tabs on screen: the one each group shows. */
  get shown(): readonly Tab[] {
    return groups(this.#layout).map((group) => group.shown);
  }

  /** How the window is divided among the tabs; `null` only when there are no tabs. */
  get layout(): Layout<Tab> | null {
    return this.#layout;
  }

  /** The tab the keyboard works on, shown in its group; `null` only when there are no tabs. */
  get active(): Tab | null {
    return this.#active;
  }

  /** True while the tabs of the previous launch are reopening. */
  get restoring(): boolean {
    return this.#restoring;
  }

  /**
   * Opens the repository containing `path` in a new tab at the end of the active tab's group, or
   * finds the tab that has it, and activates the tab. On failure the tabs stay as they were.
   */
  async open(path: string): Promise<Tab> {
    const tab = this.#add(await this.#backend.open(path));
    this.activate(tab);
    return tab;
  }

  /**
   * Reopens the tabs remembered from the previous launch, laid out as they were, and activates the
   * one that was active. Repositories that fail to open are dropped (and the first failure is
   * thrown once the others are open). Tabs opened meanwhile stay, ahead of the restored ones in the
   * first group.
   */
  async restore(): Promise<void> {
    const saved = this.#memory.tabs;
    if (saved.paths.length === 0) return;
    this.#restoring = true;
    let results: PromiseSettledResult<{ session: SessionId; info: RepoInfo }>[];
    try {
      results = await Promise.allSettled(saved.paths.map((path) => this.#backend.open(path)));
    } finally {
      this.#restoring = false;
    }

    const ahead = this.all;
    const restored = results.map((result) => (result.status === "fulfilled" ? this.#add(result.value) : null));
    let layout = restoreLayout(
      saved.layout ?? null,
      restored.map((tab) => (tab && ahead.includes(tab) ? null : tab)),
    );
    if (layout && ahead.length > 0) {
      const first = groups(layout)[0].id;
      for (const tab of [...ahead].reverse()) layout = addTab(layout, tab, { group: first, index: 0 });
    }
    layout ??= this.#layout;
    // A tab the user opened meanwhile stays active.
    const active = this.#active ?? restored[saved.active ?? -1] ?? restored.find((tab) => tab !== null) ?? null;
    this.#layout = layout && active ? show(layout, active) : layout;
    this.#active = active;
    this.#remember();

    const failure = results.find((result) => result.status === "rejected");
    if (failure) throw failure.reason;
  }

  /** Shows `tab` in its group and makes it the active tab. */
  activate(tab: Tab): void {
    if (tab === this.#active || !this.#layout || !groupOf(this.#layout, tab)) return;
    this.#layout = show(this.#layout, tab);
    this.#active = tab;
    this.#remember();
  }

  /**
   * Activates the tab `by` places after the active one (negative: before) in reading order,
   * wrapping around. From a group's last tab that is the next group's first.
   */
  cycle(by: number): void {
    const all = this.all;
    if (all.length === 0 || !this.#active) return;
    const index = all.indexOf(this.#active);
    this.activate(all[(((index + by) % all.length) + all.length) % all.length]);
  }

  /**
   * Moves `tab` to `target` and activates it: to another place among its group's tabs, into another
   * group, or into a new group beside one, which takes half of that group's area. A group left
   * without tabs goes away. A move that can't be made (see `moveTab`) only activates the tab.
   */
  move(tab: Tab, target: Target): void {
    if (!this.#layout || !groupOf(this.#layout, tab)) return;
    this.#layout = show(moveTab(this.#layout, tab, target), tab);
    this.#active = tab;
    this.#remember();
  }

  /**
   * Moves the boundary after child `index` of split `split` to `at`, a share of the split, leaving
   * its neighbors at least `min` of the split each (see `resize` in `layout.ts`).
   */
  resize(split: number, index: number, at: number, min: number): void {
    if (!this.#layout) return;
    this.#layout = resize(this.#layout, split, index, at, min);
    this.#remember();
  }

  /**
   * Closes `tab`. Its group shows the tab to its right instead, or the one to its left if it was the
   * last; a group left without tabs goes away. If the tab was active, the tab shown in its place
   * becomes active: its group's, or else that of the group now at its group's place in reading
   * order. Closing a closed tab does nothing.
   */
  close(tab: Tab): Promise<void> {
    const before = groups(this.#layout);
    const group = before.find((each) => each.tabs.includes(tab));
    if (!this.#layout || !group) return Promise.resolve();
    this.#layout = removeTab(this.#layout, tab);
    if (this.#active === tab) {
      const after = groups(this.#layout);
      const heir = after.find((each) => each.id === group.id) ?? after[Math.min(before.indexOf(group), after.length - 1)];
      this.#active = heir?.shown ?? null;
    }
    this.#remember();
    return tab.close();
  }

  /** Routes a change event to the tab of `session`; events for closed sessions are dropped. */
  adopt(session: SessionId, info: RepoInfo): void {
    this.all.find((tab) => tab.session === session)?.adopt(info);
  }

  /** Routes an operation's progress line to the tab of `session`. */
  progress(session: SessionId, id: string, text: string): void {
    this.all.find((tab) => tab.session === session)?.operations.progress(id, text);
  }

  /** The tab of an opened repository: the one that has it, or a new one at the end of the active tab's group. */
  #add(opened: { session: SessionId; info: RepoInfo }): Tab {
    const existing = this.all.find((tab) => tab.session === opened.session);
    if (existing) {
      existing.adopt(opened.info);
      return existing;
    }
    const tab = new Tab(opened.session, opened.info, this.#backend.client(opened.session));
    const group = this.#active && groupOf(this.#layout, this.#active);
    this.#layout = addTab(this.#layout, tab, group ? { group: group.id } : undefined);
    return tab;
  }

  #remember(): void {
    // Mid-restore, the list isn't complete yet; restoring remembers the whole list once it's done.
    if (this.#restoring) return;
    const all = this.all;
    const active = this.#active ? all.indexOf(this.#active) : -1;
    const saved: SavedTabs = { paths: all.map((tab) => tab.info.root), active: active < 0 ? null : active };
    // Tabs saved without a layout are one group.
    if (this.#layout?.kind === "split") saved.layout = saveLayout(this.#layout, all);
    this.#memory.tabs = saved;
  }
}
