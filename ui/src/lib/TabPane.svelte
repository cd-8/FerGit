<!--
  One tab's repository: its toolbar, what the repository is in the middle of, the graph, the diff
  opened over it, the details panel, the find bar, the filter picker, the graph's context menu, and
  its pull requests, shown over all of those when opened.
  Every open tab keeps its pane mounted, hidden by the app while another tab of its group is shown,
  so switching back finds the graph scrolled, the files focused and the diff open exactly as they were.
-->
<script lang="ts">
  import { tick, untrack } from "svelte";
  import { perform } from "./actions";
  import type { FileChange, Filter } from "./bindings";
  import ContextMenu from "./ContextMenu.svelte";
  import DetailsPanel from "./DetailsPanel.svelte";
  import DiffView from "./DiffView.svelte";
  import { describeFilter, isFiltered } from "./filter";
  import FilterPanel from "./FilterPanel.svelte";
  import FindBar from "./FindBar.svelte";
  import GraphView from "./GraphView.svelte";
  import { subjectOf, type FileList } from "./inspector.svelte";
  import type { Command, KeyContext } from "./keys";
  import { menuFor, type MenuContext, type MenuEntry, type MenuRequest } from "./menu";
  import OperationStatus from "./OperationStatus.svelte";
  import ReviewView from "./ReviewView.svelte";
  import { settings } from "./settings.svelte";
  import StateBanner from "./StateBanner.svelte";
  import type { Tab } from "./tabs.svelte";

  interface Props {
    tab: Tab;
    /** Whether this tab is the one its group shows. */
    shown: boolean;
  }

  let { tab, shown }: Props = $props();

  /** Where focus was when a diff opened; it goes back there when the diff closes. */
  let focusBeforeDiff: HTMLElement | null = null;
  let graphView = $state<ReturnType<typeof GraphView>>();
  let detailsPanel = $state<ReturnType<typeof DetailsPanel>>();
  let diffView = $state<ReturnType<typeof DiffView>>();
  let findBar = $state<ReturnType<typeof FindBar>>();
  let reviewView = $state<ReturnType<typeof ReviewView>>();
  /** The open context menu; where focus was before it opened goes back there when it closes. */
  let menu = $state.raw<{ x: number; y: number; entries: MenuEntry[]; returnFocus: HTMLElement | null } | null>(null);

  function openMenu({ row, ref, x, y }: MenuRequest): void {
    const entries = menuFor(row, ref, menuContext());
    if (entries.length === 0) return;
    const returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    menu = { x, y, entries, returnFocus };
  }

  /** What the menu needs to know about the repository: the current branch, and what is going on. */
  function menuContext(): MenuContext {
    const { info, view } = tab;
    const comparison = view.comparison;
    const [older, newer] = comparison ? [view.row(comparison.older), view.row(comparison.newer)] : [];
    const commits = older?.kind === "commit" && newer?.kind === "commit";
    return {
      branch: info.branch,
      head: info.head,
      busy: info.state.kind !== "clean",
      compared: commits ? { older: older.id, newer: newer.id } : null,
    };
  }

  function closeMenu(): void {
    menu?.returnFocus?.focus({ preventScroll: true });
    menu = null;
  }

  // A tab sent to the back closes its menu, rather than showing it again when it comes back.
  $effect(() => {
    if (!shown) untrack(() => (menu = null));
  });

  // Tell the inspector what is selected, and when the repository may have changed (including
  // refreshes that leave the generation alone, which matter for the index and worktree).
  $effect(() => {
    const { view, inspector } = tab;
    const subject = subjectOf(view);
    const head = tab.info.head;
    const version = `${view.generation}:${tab.updates}`;
    untrack(() => inspector.show(subject, head, version));
  });

  // The finder searches again whenever the rows on screen move to another snapshot (a refresh, a
  // filter), so its matches are always rows of what is shown.
  $effect(() => {
    void tab.view.generation;
    untrack(() => tab.finder.sync());
  });

  /** Opens the find bar (closing the diff over the graph, and the review) and focuses it. */
  async function openFind(): Promise<void> {
    if (tab.review.open) tab.review.toggle(false);
    if (tab.inspector.diff) closeDiff();
    tab.finder.show();
    await tick();
    findBar?.focus();
  }

  function closeFind(): void {
    tab.finder.hide();
    graphView?.focus();
  }

  /** Applies `filter` to this tab, closing the filter picker. */
  function applyFilter(filter: Filter): void {
    tab.filterOpen = false;
    void tab.setFilter(filter);
    graphView?.focus();
  }

  function openFile(list: FileList, file: FileChange): void {
    if (!tab.inspector.diff) {
      focusBeforeDiff = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    }
    tab.inspector.openFile(list, file);
  }

  function closeDiff(): void {
    tab.inspector.closeDiff();
    focusBeforeDiff?.focus();
    focusBeforeDiff = null;
  }

  /** Shows or hides the pull requests, focusing what is shown. */
  async function toggleReview(open = !tab.review.open): Promise<void> {
    tab.review.toggle(open);
    await tick();
    focus();
  }

  /** Focuses what the keyboard works on: the review, else the open diff, else the graph. */
  export function focus(): void {
    if (tab.review.open) reviewView?.focus();
    else if (tab.inspector.diff) diffView?.focus();
    else graphView?.focus();
  }

  /** The diff the keyboard scrolls: the review's, while the review is shown. */
  function activeDiff(): ReturnType<typeof DiffView> | undefined {
    return tab.review.open ? reviewView?.diff() : diffView;
  }

  /**
   * Esc, pressed in `context`: closes the find bar being typed in, else the filter picker, else the
   * review's diff or the review, else the diff, else leaves compare mode, else closes the find bar.
   */
  export function escape(context: KeyContext): void {
    if (context === "find") closeFind();
    else if (tab.filterOpen) tab.filterOpen = false;
    else if (tab.review.open) reviewView?.escape();
    else if (tab.inspector.diff) closeDiff();
    else if (tab.view.compared !== null) tab.view.compare(null);
    else if (tab.finder.open) closeFind();
  }

  /** Carries out a key command that belongs to a pane, in the pane the key was pressed in. */
  export function run(command: Command, context: KeyContext): void {
    const view = tab.view;
    switch (command.kind) {
      case "close":
        if (tab.review.open) reviewView?.escape();
        else closeDiff();
        return;
      case "open":
        detailsPanel?.openFocused();
        return;
      case "menu":
        graphView?.openMenu();
        return;
      case "find":
        void openFind();
        return;
      case "findNext":
        void tab.finder.step(command.by);
        return;
      case "focus":
        if (command.pane === "files") void focusFileList();
        else focus();
        return;
      case "scrollX":
        activeDiff()?.scrollColumns(command.by);
        return;
      case "lineEdge":
        activeDiff()?.scrollToEdge(command.edge);
        return;
      case "move":
        if (context === "files") detailsPanel?.moveFile(command.by);
        else if (context === "diff") activeDiff()?.scrollLines(command.by);
        else view.moveSelection(command.by);
        return;
      case "page":
        if (context === "diff") activeDiff()?.scrollPages(command.by);
        else view.pageSelection(command.by);
        return;
      case "goto":
        if (context === "files") detailsPanel?.gotoFile(command.row);
        else if (context === "diff") activeDiff()?.scrollToRow(command.row);
        else view.selectRow(command.row);
        return;
    }
  }

  /** Opens the details panel if needed (selecting the top row if nothing is) and focuses its files. */
  async function focusFileList(): Promise<void> {
    if (tab.view.selected === null) tab.view.moveSelection(1);
    tab.detailsOpen = true;
    await tick();
    detailsPanel?.focusFiles();
  }
</script>

<div class="pane">
  <div class="topbar">
    <span class="repo-root" title={tab.info.root}>{tab.info.root}</span>
    <span class="row-count">{tab.view.total.toLocaleString()} rows</span>
    {#if isFiltered(tab.info.filter)}
      <span class="filter-chip" title="Showing only these commits">
        <span class="filter-text">{describeFilter(tab.info.filter)}</span>
        <button
          class="icon-button"
          aria-label="Show all commits"
          title="Show all commits"
          onclick={() => applyFilter({ refs: [], path: null })}>×</button
        >
      </span>
    {/if}
    <button
      class="button"
      class:active={isFiltered(tab.info.filter)}
      aria-expanded={tab.filterOpen}
      onclick={() => (tab.filterOpen = !tab.filterOpen)}
      title="Show only some branches, or commits changing a path"
    >
      Filter…
    </button>
    <button class="button" onclick={() => void openFind()} title="Find commits (Ctrl+F, /)">Find</button>
    <label class="toggle" title="Label where branches split and join on the graph (names are inferred)">
      <input type="checkbox" bind:checked={settings.showRelations} />
      Relationships
    </label>
    {#if tab.review.host}
      <button
        class="button"
        class:active={tab.review.open}
        aria-pressed={tab.review.open}
        onclick={() => void toggleReview()}
        title="Review the pull requests of {tab.review.host.organization}/{tab.review.host.project} on Azure DevOps"
      >
        Pull requests
      </button>
    {/if}
    <button
      class="button"
      onclick={() => void perform(tab, { kind: "fetch", remote: null })}
      title="Fetch every remote, removing remote branches that were deleted there"
    >
      Fetch
    </button>
    <button
      class="button"
      onclick={() => void perform(tab, { kind: "undo" })}
      title="Undo the last operation made here from FerGit, after saying what it restores (u)"
    >
      Undo
    </button>
    <button class="button" onclick={() => tab.refresh()} disabled={tab.refreshing} title="Re-read the repository">
      Refresh
    </button>
  </div>
  <StateBanner
    repoState={tab.info.state}
    busy={tab.operations.active.length > 0}
    onaction={(kind) => void perform(tab, { kind })}
  />
  <OperationStatus operations={tab.operations} />

  <div class="content">
    <div class="workspace">
      <!-- The graph stays laid out under an open diff, so closing the diff finds it unchanged. -->
      <div class="graph-layer" inert={tab.inspector.diff !== null || tab.review.open}>
        <GraphView
          bind:this={graphView}
          view={tab.view}
          highlight={tab.finder.open ? tab.finder : undefined}
          onactivate={() => (tab.detailsOpen = true)}
          onmenu={openMenu}
        />
        {#if tab.finder.open}
          <FindBar bind:this={findBar} finder={tab.finder} onclose={closeFind} />
        {/if}
      </div>
      {#if tab.inspector.diff}
        <div class="diff-layer">
          {#key tab.inspector.diff.request}
            <DiffView bind:this={diffView} diff={tab.inspector.diff} onclose={closeDiff} />
          {/key}
        </div>
      {/if}
    </div>
    {#if tab.detailsOpen && tab.view.selected !== null}
      <DetailsPanel
        bind:this={detailsPanel}
        view={tab.view}
        inspector={tab.inspector}
        onopen={openFile}
        onclose={() => (tab.detailsOpen = false)}
      />
    {/if}
    {#if tab.filterOpen}
      <FilterPanel
        current={tab.info.filter}
        loadRefs={() => tab.client.refs()}
        onapply={applyFilter}
        onclose={() => (tab.filterOpen = false)}
      />
    {/if}
    {#if tab.review.open}
      <div class="review-layer">
        <ReviewView bind:this={reviewView} review={tab.review} onclose={() => void toggleReview(false)} />
      </div>
    {/if}
  </div>
  {#if menu}
    <ContextMenu x={menu.x} y={menu.y} entries={menu.entries} onpick={(action) => void perform(tab, action)} onclose={closeMenu} />
  {/if}
</div>

<style>
  /* Fills the area the app gives the tab's group. */
  .pane {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
  }

  /* In a narrow pane the buttons wrap onto a second line, leaving the path room to be read. */
  .topbar {
    display: flex;
    flex: none;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 10px;
    min-height: 36px;
    padding: 4px 10px;
    border-bottom: 1px solid var(--border);
  }

  .repo-root {
    flex: 1 1 140px;
    min-width: 0;
    overflow: hidden;
    color: var(--fg-muted);
    font-size: 12px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .toggle {
    display: flex;
    flex: none;
    align-items: center;
    gap: 5px;
    color: var(--fg-muted);
    font-size: 12px;
    white-space: nowrap;
    user-select: none;
  }

  .toggle input {
    margin: 0;
    accent-color: var(--primary-bg);
  }

  .row-count {
    flex: none;
    color: var(--fg-subtle);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }

  .button.active {
    border-color: var(--focus);
    color: var(--focus);
  }

  .filter-chip {
    display: flex;
    flex: 0 1 auto;
    align-items: center;
    gap: 2px;
    min-width: 0;
    max-width: 280px;
    padding: 0 0 0 8px;
    border: 1px solid var(--focus);
    border-radius: 12px;
    background: color-mix(in srgb, var(--focus) 10%, transparent);
    color: var(--fg);
    font-size: 12px;
  }

  .filter-text {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .filter-chip .icon-button {
    width: 20px;
    height: 20px;
    border-radius: 10px;
    font-size: 14px;
  }

  /* What the filter picker is placed in. */
  .content {
    position: relative;
    display: flex;
    flex: 1;
    min-height: 0;
  }

  .workspace {
    position: relative;
    display: flex;
    flex: 1;
    min-width: 0;
  }

  .graph-layer {
    position: relative;
    display: flex;
    flex: 1;
    min-width: 0;
  }

  .diff-layer {
    position: absolute;
    inset: 0;
    z-index: 3;
    display: flex;
    background: var(--bg);
  }

  /* Over the graph, its diff and the panels beside it, which stay as they were underneath. */
  .review-layer {
    position: absolute;
    inset: 0;
    z-index: 5;
    display: flex;
    background: var(--bg);
  }
</style>
