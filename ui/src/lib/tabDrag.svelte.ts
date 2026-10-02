import type { Rect, Target } from "./layout";
import type { Tab } from "./tabs.svelte";

/** How far the pointer goes with the button down before a press on a tab becomes a drag. */
const DRAG_THRESHOLD_PX = 6;

/** Where a dragged tab would land, and the part of the window to mark for it, in client pixels. */
export interface Drop {
  target: Target;
  area: Rect;
}

/** A drag under way: the tab, where the pointer is, and where the tab would land if let go now. */
export interface Dragging {
  tab: Tab;
  x: number;
  y: number;
  drop: Drop | null;
}

/**
 * Dragging a tab with the pointer to another place in the window.
 *
 * A press becomes a drag once the pointer has moved a little, so clicks stay clicks. From then on
 * `current` follows the pointer, and letting go over a place where the tab can land hands it to
 * `ondrop`. The pointer is followed on the window rather than captured, which leaves the click of a
 * press that never became a drag to the tab's own button.
 */
export class TabDrag {
  readonly #dropAt: (tab: Tab, x: number, y: number) => Drop | null;
  readonly #ondrop: (tab: Tab, target: Target) => void;
  #current = $state.raw<Dragging | null>(null);
  #release: (() => void) | null = null;

  /**
   * `dropAt` says where `tab` would land if let go at a point of the window, `null` for nowhere;
   * `ondrop` moves it there.
   */
  constructor(dropAt: (tab: Tab, x: number, y: number) => Drop | null, ondrop: (tab: Tab, target: Target) => void) {
    this.#dropAt = dropAt;
    this.#ondrop = ondrop;
  }

  /** The drag under way, if any. Reactive. */
  get current(): Dragging | null {
    return this.#current;
  }

  /** Call when the main button goes down on `tab`. */
  press(tab: Tab, event: PointerEvent): void {
    this.cancel();
    const { pointerId, clientX: startX, clientY: startY } = event;
    const move = (moved: PointerEvent) => {
      if (moved.pointerId !== pointerId) return;
      const { clientX: x, clientY: y } = moved;
      if (!this.#current && Math.hypot(x - startX, y - startY) < DRAG_THRESHOLD_PX) return;
      this.#current = { tab, x, y, drop: this.#dropAt(tab, x, y) };
    };
    const up = (released: PointerEvent) => {
      if (released.pointerId !== pointerId) return;
      const drop = this.#current?.drop;
      this.cancel();
      if (drop) this.#ondrop(tab, drop.target);
    };
    const cancel = () => this.cancel();
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", cancel);
    window.addEventListener("blur", cancel);
    this.#release = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", cancel);
      window.removeEventListener("blur", cancel);
    };
  }

  /** Stops following the pointer; a drag under way ends without moving the tab. */
  cancel(): void {
    this.#release?.();
    this.#release = null;
    this.#current = null;
  }
}
