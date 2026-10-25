import { sidebarAnchorScrollTarget, sidebarScrollAnchorLayout, sidebarScrollGesture, sidebarTrailingSpaceAfterScroll, type SidebarScrollGesture, type SidebarScrollGestureEvent } from "./sidebarScrollAnchorModel";
import { observeSidebarRows, sidebarScrollDebug } from "./sidebarScrollDebug";

const ANCHORS = ".sidebar-structural-header, .sidebar-workspace-card-scroll, .sidebar-workspace-detail-header, [data-testid=workspace-navigation-actions], .sidebar-team-header, .sidebar-team-workers-toggle, .sidebar-tab";

interface Anchor {
  element: HTMLElement;
  y: number;
}

export function retainSidebarScrollAnchor(viewport: HTMLElement, body: HTMLElement, space: HTMLElement): () => void {
  let anchors: Anchor[] = [];
  let previousScrollTop = viewport.scrollTop;
  let previousMaximum = viewport.scrollHeight - viewport.clientHeight;
  let preferred: HTMLElement | null = null;
  let disposed = false;
  let dragging = false;
  let gesture: SidebarScrollGesture = { phase: "idle", moved: false };
  const stopRowDebug = observeSidebarRows(viewport, body);
  const scale = () => viewport.getBoundingClientRect().height / viewport.offsetHeight || 1;
  const y = (element: HTMLElement) => (element.getBoundingClientRect().top - viewport.getBoundingClientRect().top) / scale();

  const capture = () => {
    previousScrollTop = viewport.scrollTop;
    previousMaximum = viewport.scrollHeight - viewport.clientHeight;
    const bounds = viewport.getBoundingClientRect();
    const visible = [...body.querySelectorAll<HTMLElement>(ANCHORS)].filter((element) => {
      const rect = element.getBoundingClientRect();
      return rect.height > 0 && rect.bottom > bounds.top && rect.top < bounds.bottom
        && (element === preferred || getComputedStyle(element).position !== "sticky");
    });
    if (preferred && visible.includes(preferred)) {
      visible.splice(visible.indexOf(preferred), 1);
      visible.unshift(preferred);
    }
    anchors = visible.map((element) => ({ element, y: y(element) }));
  };

  const apply = (desiredScrollTop: number) => {
    const contentHeight = viewport.scrollHeight - space.offsetHeight;
    const layout = sidebarScrollAnchorLayout(desiredScrollTop, contentHeight, viewport.clientHeight);
    space.style.height = `${layout.trailingSpace}px`;
    if (layout.scrollTop !== viewport.scrollTop) {
      sidebarScrollDebug(viewport, "write", { desiredScrollTop: layout.scrollTop, phase: gesture.phase });
      viewport.scrollTop = layout.scrollTop;
    }
  };

  const releaseTrailingSpace = () => {
    const contentHeight = viewport.scrollHeight - space.offsetHeight;
    const trailingSpace = sidebarTrailingSpaceAfterScroll(viewport.scrollTop, contentHeight, viewport.clientHeight, space.offsetHeight);
    if (trailingSpace !== space.offsetHeight) space.style.height = `${trailingSpace}px`;
  };

  const restore = () => {
    if (disposed) return;
    if (gesture.phase !== "idle") {
      sidebarScrollDebug(viewport, "native-owned", { phase: gesture.phase });
      capture();
      return;
    }
    if (body.querySelector('[data-dnd-dragging="true"]')) {
      dragging = true;
      return;
    }
    if (dragging) {
      dragging = false;
      capture();
      return;
    }
    if (getComputedStyle(viewport).overflowY === "visible") {
      space.style.height = "0px";
      capture();
      return;
    }
    const anchor = anchors.find(({ element }) => body.contains(element) && element.getClientRects().length > 0);
    if (anchor) {
      const currentY = y(anchor.element);
      const desired = sidebarAnchorScrollTarget(
        { y: anchor.y, scrollTop: previousScrollTop, maximum: previousMaximum },
        { y: currentY, scrollTop: viewport.scrollTop, maximum: viewport.scrollHeight - viewport.clientHeight },
        false,
      );
      sidebarScrollDebug(viewport, "restore", { desired, capturedY: anchor.y, currentY, capturedScrollTop: previousScrollTop });
      if (desired !== null) apply(desired);
      else releaseTrailingSpace();
    } else {
      const maximum = Math.max(0, viewport.scrollHeight - space.offsetHeight - viewport.clientHeight);
      apply(Math.min(viewport.scrollTop, maximum));
    }
    capture();
  };

  const onScroll = () => {
    preferred = null;
    if (gesture.phase === "idle") releaseTrailingSpace();
    capture();
  };
  const onPress = (event: Event) => {
    if (event.type === "keydown" || (event as PointerEvent).pointerType !== "touch") {
      gesture = sidebarScrollGesture(gesture, "press");
    }
    preferred = event.target instanceof Element ? event.target.closest<HTMLElement>(ANCHORS) : null;
    capture();
  };
  const onGesture = (event: Event) => {
    if (event.type === "touchend" && (event as TouchEvent).touches?.length > 0) return;
    gesture = sidebarScrollGesture(gesture, event.type as SidebarScrollGestureEvent);
    sidebarScrollDebug(viewport, event.type, { phase: gesture.phase });
    capture();
  };
  const gestureEvents = ["touchstart", "touchmove", "touchend", "touchcancel", "scrollend"];
  const mutation = new MutationObserver(restore);
  // Text and attribute churn (activity dots, statuses) never moves rows; size changes reach the ResizeObserver.
  mutation.observe(body, { childList: true, subtree: true, attributes: true, attributeFilter: ["data-dnd-dragging"] });
  const resize = new ResizeObserver(restore);
  resize.observe(body);
  resize.observe(viewport);
  viewport.addEventListener("scroll", onScroll, { passive: true });
  viewport.addEventListener("pointerdown", onPress, true);
  viewport.addEventListener("keydown", onPress, true);
  for (const event of gestureEvents) viewport.addEventListener(event, onGesture, { passive: true, capture: true });
  capture();
  return () => {
    disposed = true;
    stopRowDebug();
    mutation.disconnect();
    resize.disconnect();
    viewport.removeEventListener("scroll", onScroll);
    viewport.removeEventListener("pointerdown", onPress, true);
    viewport.removeEventListener("keydown", onPress, true);
    for (const event of gestureEvents) viewport.removeEventListener(event, onGesture, true);
    space.style.height = "0px";
  };
}
