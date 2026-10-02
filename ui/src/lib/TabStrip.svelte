<!--
  The tab strip of one group: its tabs, with the one the group shows marked, and a button to open
  another repository there. A press on a tab is reported as it happens, because it may be the start
  of a drag (see `TabDrag`); a click still shows the tab. Each tab carries `data-tab`, its session,
  for whoever needs to find where the tabs are.
-->
<script lang="ts">
  import type { Snippet } from "svelte";
  import type { Tab } from "./tabs.svelte";

  interface Props {
    tabs: readonly Tab[];
    /** The tab the group shows; `null` only without tabs. */
    shown: Tab | null;
    /** Whether the shown tab is the one the keyboard works on. */
    focused: boolean;
    onshow: (tab: Tab) => void;
    onclose: (tab: Tab) => void;
    /** The user asked to open a repository in this group. */
    onopen: () => void;
    /** The main button went down on a tab's name. */
    onpress: (tab: Tab, event: PointerEvent) => void;
    /** Controls for the far end of the strip. */
    end?: Snippet;
  }

  let { tabs, shown, focused, onshow, onclose, onopen, onpress, end }: Props = $props();

  /** Middle-click closes a tab; stopping the press keeps the browser from starting autoscroll. */
  function onmousedown(event: MouseEvent): void {
    if (event.button === 1) event.preventDefault();
  }

  function onauxclick(event: MouseEvent, tab: Tab): void {
    if (event.button === 1) onclose(tab);
  }
</script>

<header class="tabstrip" class:focused>
  <div class="tabs" role="tablist" aria-label="Open repositories">
    {#each tabs as tab (tab.session)}
      <div class="tab" class:active={tab === shown} data-tab={tab.session}>
        <button
          class="tab-label"
          role="tab"
          aria-selected={tab === shown}
          title={tab.info.root}
          onclick={() => onshow(tab)}
          onpointerdown={(event) => event.button === 0 && onpress(tab, event)}
          {onmousedown}
          onauxclick={(event) => onauxclick(event, tab)}
        >
          {tab.info.name}
        </button>
        <button
          class="icon-button tab-close"
          aria-label="Close {tab.info.name}"
          title="Close (Ctrl+W)"
          onclick={() => onclose(tab)}>×</button
        >
      </div>
    {/each}
  </div>
  <button
    class="icon-button"
    aria-label="Open repository in a new tab"
    title="Open repository (Ctrl+T)"
    onclick={onopen}>+</button
  >
  <span class="spacer"></span>
  {@render end?.()}
</header>

<style>
  .tabstrip {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 100%;
    padding: 0 8px 0 0;
    border-bottom: 1px solid var(--border);
    background: var(--bg-subtle);
  }

  .tabs {
    display: flex;
    align-self: stretch;
    min-width: 0;
    overflow-x: auto;
    scrollbar-width: none;
  }

  .tab {
    display: flex;
    flex: 0 1 auto;
    align-items: center;
    min-width: 72px;
    max-width: 220px;
    padding-right: 4px;
    border-right: 1px solid var(--border);
    color: var(--fg-muted);
  }

  .tab:hover {
    background: var(--bg-hover);
  }

  /* The shown tab joins the content below it. Only the one the keyboard works on is accented. */
  .tab.active {
    margin-bottom: -1px;
    background: var(--bg);
    color: var(--fg);
    box-shadow: inset 0 2px 0 var(--border-strong);
  }

  .focused .tab.active {
    box-shadow: inset 0 2px 0 var(--primary-bg);
  }

  .tab-label {
    flex: 1;
    min-width: 0;
    height: 100%;
    padding: 0 6px 0 12px;
    overflow: hidden;
    border: 0;
    background: transparent;
    text-align: left;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: default;
  }

  .focused .tab.active .tab-label {
    font-weight: 600;
  }

  .tab-close {
    flex: none;
    width: 20px;
    height: 20px;
    font-size: 14px;
  }

  /* Only the shown tab and the one under the mouse show their close button. */
  .tab:not(.active):not(:hover) .tab-close:not(:focus-visible) {
    visibility: hidden;
  }

  .spacer {
    flex: 1;
  }
</style>
