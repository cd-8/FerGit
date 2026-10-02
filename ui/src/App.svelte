<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { open } from "@tauri-apps/plugin-dialog";
  import { tick } from "svelte";
  import { followCredentialPrompts, perform } from "./lib/actions";
  import Dialog from "./lib/Dialog.svelte";
  import { dialogs } from "./lib/dialogs.svelte";
  import KeyHelp from "./lib/KeyHelp.svelte";
  import { KeyInterpreter, PENDING_G_TIMEOUT_MS, type Command, type KeyContext } from "./lib/keys";
  import { arrange, dropZone, groupOf, topRight, zoneArea, type Group, type Sash } from "./lib/layout";
  import { session } from "./lib/session.svelte";
  import { TabDrag, type Drop } from "./lib/tabDrag.svelte";
  import TabPane from "./lib/TabPane.svelte";
  import TabStrip from "./lib/TabStrip.svelte";
  import type { Tab } from "./lib/tabs.svelte";
  import { theme } from "./lib/settings.svelte";
  import { applyTheme, nextThemeMode, type ThemeMode } from "./lib/theme.svelte";

  const tabs = session.tabs;
  /** Each open tab's pane, by session. */
  const panes: Record<number, ReturnType<typeof TabPane> | undefined> = $state({});
  const activePane = $derived(tabs.active ? panes[tabs.active.session] : undefined);

  // The window: each group's tab strip over the pane of the tab it shows, and between neighboring
  // groups a boundary to drag. All of it is placed by the layout's arrangement, in shares of the
  // window, so nothing is ever mounted again because the layout changed.
  /** The least a boundary being dragged leaves the groups on either side of it, across it. */
  const MIN_GROUP_PX = 200;
  let main = $state<HTMLElement>();
  const arrangement = $derived(arrange(tabs.layout));
  /** The group whose strip holds the window's own buttons. */
  const corner = $derived(tabs.layout && topRight(tabs.layout));
  /**
   * Every tab with its group and the group's area. In session order whatever the layout: a pane
   * moved to another place in the page would lose its scroll positions.
   */
  const slots = $derived(
    arrangement.groups
      .flatMap(({ group, rect }) => group.tabs.map((tab) => ({ tab, group, rect })))
      .sort((a, b) => a.tab.session - b.tab.session),
  );
  const percent = (share: number): string => `${share * 100}%`;

  const drag = new TabDrag(dropAt, (tab, target) => {
    tabs.move(tab, target);
    void focusActive();
  });

  /**
   * Where `tab` would land if dropped at a point of the window. Over a tab strip: among its tabs,
   * before the first whose middle is past the point. Over a pane: beside its group or into it.
   */
  function dropAt(tab: Tab, x: number, y: number): Drop | null {
    const under = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-group]");
    if (!under) return null;
    const group = Number(under.dataset.group);
    const box = under.getBoundingClientRect();
    if (under.classList.contains("strip")) {
      const others = [...under.querySelectorAll<HTMLElement>("[data-tab]")]
        .filter((other) => Number(other.dataset.tab) !== tab.session)
        .map((other) => other.getBoundingClientRect());
      const index = others.filter((other) => other.left + other.width / 2 < x).length;
      const edge = index < others.length ? others[index].left : (others.at(-1)?.right ?? box.left);
      const line = { x: Math.max(edge - 1, box.left), y: box.top, width: 2, height: box.height };
      return { target: { group, zone: "center", index }, area: line };
    }
    // A group's only tab has nothing to go beside in its own group.
    const from = groupOf(tabs.layout, tab);
    const alone = from?.id === group && from.tabs.length === 1;
    const zone = alone ? "center" : dropZone((x - box.left) / box.width, (y - box.top) / box.height);
    return { target: { group, zone }, area: zoneArea(zone, box) };
  }

  /** Moves the boundary being dragged to the pointer. */
  function dragSash(sash: Sash, event: PointerEvent): void {
    if (!main) return;
    const box = main.getBoundingClientRect();
    const [pointer, start, length] =
      sash.axis === "row"
        ? [event.clientX, box.left + sash.rect.x * box.width, sash.rect.width * box.width]
        : [event.clientY, box.top + sash.rect.y * box.height, sash.rect.height * box.height];
    tabs.resize(sash.split, sash.index, (pointer - start) / length, MIN_GROUP_PX / length);
  }

  // Keyboard: every shortcut goes through here. The interpreter turns presses into commands; `run`
  // carries each one out, in the pane of the active tab when it belongs to a pane.
  const keys = new KeyInterpreter();
  /** The half-typed sequence (a count, a `g`), shown like vim's showcmd. */
  let pendingKeys = $state("");
  let helpOpen = $state(false);
  let pendingTimer: ReturnType<typeof setTimeout> | undefined;

  function onkeydown(event: KeyboardEvent): void {
    if (event.defaultPrevented) return;
    if (drag.current && event.key === "Escape") {
      drag.cancel();
      event.preventDefault();
      return;
    }
    const context = keyContext(event.target);
    const result = keys.press(
      {
        key: event.key,
        ctrlKey: event.ctrlKey,
        shiftKey: event.shiftKey,
        altKey: event.altKey,
        metaKey: event.metaKey,
        editable: isEditable(event.target),
        context,
      },
      event.timeStamp,
    );
    if (result.handled) event.preventDefault();
    showPendingKeys();
    if (result.command) run(result.command, context);
  }

  function showPendingKeys(): void {
    pendingKeys = keys.pending;
    clearTimeout(pendingTimer);
    if (pendingKeys.endsWith("g")) {
      pendingTimer = setTimeout(() => {
        keys.expire(performance.now());
        pendingKeys = keys.pending;
      }, PENDING_G_TIMEOUT_MS + 50);
    }
  }

  /** Which pane a key press belongs to, from what has focus. */
  function keyContext(target: EventTarget | null): KeyContext {
    if (helpOpen) return "help";
    const element = target instanceof Element ? target : null;
    if (element?.closest(".find-bar")) return element.matches("input") ? "find" : "other";
    if (element?.closest(".details")) {
      // Other controls in the panel (parent links, "Show all") keep their own keys, Enter included.
      const control = element.closest("button, a, input, select, textarea");
      return control && !control.matches("button.file") ? "other" : "files";
    }
    if (element?.closest(".tabstrip, .topbar, .banner, .empty, .failure, .filter-panel")) return "other";
    return tabs.active?.inspector.diff ? "diff" : "graph";
  }

  function isEditable(target: EventTarget | null): boolean {
    if (!(target instanceof HTMLElement)) return false;
    if (target.isContentEditable || target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement) {
      return true;
    }
    const nonText = ["checkbox", "radio", "button", "submit", "reset", "range", "color", "file", "image"];
    return target instanceof HTMLInputElement && !nonText.includes(target.type);
  }

  function run(command: Command, context: KeyContext): void {
    switch (command.kind) {
      case "escape":
        if (helpOpen) helpOpen = false;
        else activePane?.escape(context);
        return;
      case "help":
        helpOpen = !helpOpen;
        return;
      case "theme":
        theme.cycle();
        return;
      case "undo":
        if (tabs.active) void perform(tabs.active, { kind: "undo" });
        return;
      case "newTab":
        void chooseRepository();
        return;
      case "closeTab":
        if (tabs.active) void closeTab(tabs.active);
        return;
      case "switchTab":
        tabs.cycle(command.by);
        void focusActive();
        return;
      default:
        activePane?.run(command, context);
    }
  }

  /** Moves focus into the active tab, so its keys work straight away. */
  async function focusActive(): Promise<void> {
    await tick();
    activePane?.focus();
  }

  function showTab(tab: Tab): void {
    tabs.activate(tab);
    void focusActive();
  }

  async function closeTab(tab: Tab): Promise<void> {
    const wasActive = tab === tabs.active;
    const closing = tabs.close(tab);
    if (wasActive) void focusActive();
    await closing;
  }

  async function openRepository(path: string): Promise<Tab> {
    const tab = await tabs.open(path);
    await focusActive();
    return tab;
  }

  /** Asks for a repository and opens it in `group`, or in the active tab's group. */
  async function chooseRepository(group?: Group<Tab>): Promise<void> {
    if (group) tabs.activate(group.shown);
    const path = await open({ directory: true, title: "Open repository" });
    if (typeof path === "string") await openRepository(path);
  }

  /** How the theme button shows each mode. */
  const THEME_LABELS: Record<ThemeMode, { icon: string; name: string }> = {
    system: { icon: "◐", name: "System" },
    light: { icon: "☀︎", name: "Light" }, // ☀ as text, not an emoji
    dark: { icon: "☾", name: "Dark" },
  };
  const themeLabel = $derived(THEME_LABELS[theme.mode]);
  const nextThemeLabel = $derived(THEME_LABELS[nextThemeMode(theme.mode)]);

  // Theme: follow the OS while the app runs, show the resolved theme on the page, and give the
  // native window (its title bar) the chosen mode, `null` meaning the OS's.
  $effect(() => theme.follow());
  $effect(() => applyTheme(document.documentElement, theme.theme));
  $effect(() => {
    const mode = theme.mode;
    // Cosmetic, so failures (no Tauri window, as with the mock backend) are ignored.
    try {
      getCurrentWindow()
        .setTheme(mode === "system" ? null : mode)
        .catch(() => {});
    } catch {
      // Not running in a Tauri window.
    }
  });

  // Follow changes the backend detects on disk, and operations' progress, for as long as the app
  // runs. Ask for the credentials git needs, whichever tab's operation needs them, in one dialog.
  $effect(() => session.followChanges());
  $effect(() => followCredentialPrompts());

  // Reopen the tabs of the previous launch.
  void tabs.restore().then(focusActive);

  // Dev-only startup hooks, for testing without the folder picker.
  if (import.meta.env.DEV && import.meta.env.VITE_FERGIT_OPEN) {
    const select = Number(import.meta.env.VITE_FERGIT_SELECT ?? "");
    void openRepository(import.meta.env.VITE_FERGIT_OPEN).then((tab) => {
      if (import.meta.env.VITE_FERGIT_SELECT && Number.isInteger(select)) {
        tab.view.select(select, true);
      }
    });
  }
</script>

<svelte:window onfocus={() => session.refreshSoon()} {onkeydown} />

{#snippet windowButtons()}
  <button
    class="icon-button theme-button"
    aria-label="Theme: {themeLabel.name}"
    title="Theme: {themeLabel.name}. Switch to {nextThemeLabel.name} (T)"
    onclick={() => theme.cycle()}>{themeLabel.icon}</button
  >
  <button
    class="icon-button keys-button"
    aria-label="Keyboard shortcuts"
    title="Keyboard shortcuts (?)"
    onclick={() => (helpOpen = !helpOpen)}>?</button
  >
{/snippet}

<div class="app" class:dragging={drag.current !== null}>
  {#if session.error}
    <div class="banner" role="alert">
      <span class="banner-message">{session.error}</span>
      <button
        class="icon-button"
        aria-label="Dismiss error"
        title="Dismiss"
        onclick={() => session.dismissError()}>×</button
      >
    </div>
  {/if}

  <main class="main" bind:this={main}>
    {#each arrangement.groups as { group, rect } (group.id)}
      <div
        class="strip"
        class:inner-left={rect.x > 0}
        class:inner-top={rect.y > 0}
        data-group={group.id}
        style:left={percent(rect.x)}
        style:top={percent(rect.y)}
        style:width={percent(rect.width)}
      >
        <TabStrip
          tabs={group.tabs}
          shown={group.shown}
          focused={group.shown === tabs.active}
          onshow={showTab}
          onclose={(tab) => void closeTab(tab)}
          onopen={() => void chooseRepository(group)}
          onpress={(tab, event) => drag.press(tab, event)}
          end={group.id === corner?.id ? windowButtons : undefined}
        />
      </div>
    {/each}

    {#each slots as { tab, group, rect } (tab.session)}
      {@const shown = tab === group.shown}
      <!-- A press or focus anywhere in a pane makes its tab the one the keyboard works on. The
           panel itself takes no focus: what is in it does. -->
      <!-- svelte-ignore a11y_interactive_supports_focus -->
      <div
        class="slot"
        class:hidden={!shown}
        class:inner-left={rect.x > 0}
        role="tabpanel"
        aria-label={tab.info.name}
        inert={!shown}
        data-group={group.id}
        style:left={percent(rect.x)}
        style:top="calc({percent(rect.y)} + var(--strip-height))"
        style:width={percent(rect.width)}
        style:height="calc({percent(rect.height)} - var(--strip-height))"
        onpointerdown={() => tabs.activate(tab)}
        onfocusin={() => tabs.activate(tab)}
      >
        <TabPane bind:this={panes[tab.session]} {tab} {shown} />
      </div>
    {/each}

    {#each arrangement.sashes as sash (`${sash.split}:${sash.index}`)}
      <div
        class="sash {sash.axis}"
        role="separator"
        aria-orientation={sash.axis === "row" ? "vertical" : "horizontal"}
        style:left={percent(sash.rect.x + (sash.axis === "row" ? sash.at * sash.rect.width : 0))}
        style:top={percent(sash.rect.y + (sash.axis === "column" ? sash.at * sash.rect.height : 0))}
        style:width={sash.axis === "column" ? percent(sash.rect.width) : undefined}
        style:height={sash.axis === "row" ? percent(sash.rect.height) : undefined}
        onpointerdown={(event) => {
          if (event.button !== 0) return;
          event.preventDefault();
          event.currentTarget.setPointerCapture(event.pointerId);
        }}
        onpointermove={(event) => {
          if (event.currentTarget.hasPointerCapture(event.pointerId)) dragSash(sash, event);
        }}
      ></div>
    {/each}

    {#if !tabs.layout}
      <div class="strip" style:left="0" style:top="0" style:width="100%">
        <TabStrip
          tabs={[]}
          shown={null}
          focused
          onshow={showTab}
          onclose={(tab) => void closeTab(tab)}
          onopen={() => void chooseRepository()}
          onpress={(tab, event) => drag.press(tab, event)}
          end={windowButtons}
        />
      </div>
      <div class="empty">
        {#if tabs.restoring}
          <p>Opening repositories…</p>
        {:else}
          <h1>No repository open</h1>
          <p>Open a folder that contains a git repository to browse its history.</p>
          <button class="button primary" onclick={() => void chooseRepository()}>Open repository…</button>
        {/if}
      </div>
    {/if}
  </main>

  {#if drag.current}
    {@const { tab, x, y, drop } = drag.current}
    {#if drop}
      <div
        class="drop-area"
        style:left="{drop.area.x}px"
        style:top="{drop.area.y}px"
        style:width="{drop.area.width}px"
        style:height="{drop.area.height}px"
      ></div>
    {/if}
    <div class="drag-label" style:left="{x + 12}px" style:top="{y + 14}px">{tab.info.name}</div>
  {/if}
  {#if pendingKeys}
    <div class="pending-keys" aria-live="polite" title="Keys typed so far">{pendingKeys}</div>
  {/if}
  {#if helpOpen}
    <KeyHelp onclose={() => (helpOpen = false)} />
  {/if}
  {#if dialogs.current}
    {#key dialogs.current}
      <Dialog spec={dialogs.current.spec} onclose={(values) => dialogs.close(values)} />
    {/key}
  {/if}
</div>

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  .app.dragging {
    user-select: none;
  }

  .app.dragging :global(*) {
    cursor: grabbing !important;
  }

  .theme-button {
    flex: none;
    font-size: 14px;
  }

  .keys-button {
    flex: none;
    font-size: 13px;
    font-weight: 600;
  }

  /* Like vim's showcmd: small, in a corner, out of the way of the mouse. */
  .pending-keys {
    position: fixed;
    right: 14px;
    bottom: 10px;
    z-index: 10;
    min-width: 24px;
    padding: 1px 8px;
    border: 1px solid var(--border-strong);
    border-radius: 4px;
    background: var(--bg-subtle);
    color: var(--fg-muted);
    font-family: var(--font-mono);
    font-size: 12px;
    text-align: center;
    pointer-events: none;
  }

  .banner {
    display: flex;
    flex: none;
    align-items: flex-start;
    gap: 8px;
    padding: 6px 6px 6px 12px;
    border-bottom: 1px solid var(--danger-border);
    background: var(--danger-bg);
    color: var(--danger-fg);
  }

  .banner-message {
    flex: 1;
    max-height: 6em;
    overflow: auto;
    padding-top: 3px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .banner .icon-button {
    color: inherit;
  }

  /* Holds the groups: for each its tab strip, and under it the panes of its tabs, stacked. */
  .main {
    --strip-height: 36px;
    position: relative;
    flex: 1;
    min-height: 0;
  }

  .strip {
    position: absolute;
    height: var(--strip-height);
  }

  /* Panes are stacked; the hidden ones keep their layout (and so their scroll positions). */
  .slot {
    position: absolute;
  }

  .slot.hidden {
    visibility: hidden;
  }

  /* A group that isn't at the window's edge is ruled off from its neighbor. */
  .inner-left {
    border-left: 1px solid var(--border);
  }

  .inner-top {
    border-top: 1px solid var(--border);
  }

  /* A boundary between groups: a strip to grab, wider than the line it lights up when pointed at. */
  .sash {
    position: absolute;
    z-index: 5;
    touch-action: none;
  }

  .sash.row {
    width: 7px;
    margin-left: -3px;
    cursor: col-resize;
  }

  .sash.column {
    height: 7px;
    margin-top: -3px;
    cursor: row-resize;
  }

  .sash::after {
    position: absolute;
    inset: 0 2px;
    background: var(--focus);
    content: "";
    opacity: 0;
    transition: opacity 0.1s;
  }

  .sash.column::after {
    inset: 2px 0;
  }

  .sash:hover::after,
  .sash:active::after {
    opacity: 1;
    transition-delay: 0.15s;
  }

  /* Where the tab being dragged would land. */
  .drop-area {
    position: fixed;
    z-index: 25;
    border: 1px solid var(--focus);
    background: color-mix(in srgb, var(--focus) 16%, transparent);
    pointer-events: none;
  }

  .drag-label {
    position: fixed;
    z-index: 26;
    max-width: 220px;
    padding: 3px 10px;
    overflow: hidden;
    border: 1px solid var(--border-strong);
    border-radius: 4px;
    background: var(--bg);
    box-shadow: 0 4px 14px var(--shadow);
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
    pointer-events: none;
  }

  .empty {
    position: absolute;
    inset: var(--strip-height) 0 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 4px;
    padding: 24px;
    color: var(--fg-muted);
    text-align: center;
  }

  .empty h1 {
    margin: 0;
    color: var(--fg);
    font-size: 16px;
    font-weight: 600;
  }

  .empty p {
    margin: 0 0 12px;
  }
</style>
