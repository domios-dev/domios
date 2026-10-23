import { sidebarAnchorShift, sidebarScrollAnchorLayout, sidebarTrailingSpaceAfterScroll } from "./sidebarScrollAnchorModel";

const ANCHORS = ".sidebar-structural-header, .sidebar-workspace-card-scroll, .sidebar-workspace-detail-header, [data-testid=workspace-navigation-actions], .sidebar-team-header, .sidebar-team-workers-toggle, .sidebar-tab";

interface Anchor {
  element: HTMLElement;
  y: number;
}

export function retainSidebarScrollAnchor(viewport: HTMLElement, body: HTMLElement, space: HTMLElement): () => void {
  let anchors: Anchor[] = [];
  let previousScrollTop = viewport.scrollTop;
  let preferred: HTMLElement | null = null;
  let disposed = false;
  let dragging = false;
  const scale = () => viewport.getBoundingClientRect().height / viewport.offsetHeight || 1;
  const y = (element: HTMLElement) => (element.getBoundingClientRect().top - viewport.getBoundingClientRect().top) / scale();

  const capture = () => {
    previousScrollTop = viewport.scrollTop;
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
    viewport.scrollTop = layout.scrollTop;
  };

  const releaseTrailingSpace = () => {
    const contentHeight = viewport.scrollHeight - space.offsetHeight;
    const trailingSpace = sidebarTrailingSpaceAfterScroll(viewport.scrollTop, contentHeight, viewport.clientHeight, space.offsetHeight);
    if (trailingSpace !== space.offsetHeight) space.style.height = `${trailingSpace}px`;
  };

  const restore = () => {
    if (disposed) return;
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
      const shift = sidebarAnchorShift(y(anchor.element), anchor.y);
      if (shift !== 0) apply(viewport.scrollTop + shift);
      else releaseTrailingSpace();
    } else {
      const maximum = Math.max(0, viewport.scrollHeight - space.offsetHeight - viewport.clientHeight);
      apply(Math.min(previousScrollTop, maximum));
    }
    capture();
  };

  const onScroll = () => {
    preferred = null;
    releaseTrailingSpace();
    capture();
  };
  const onPress = (event: Event) => {
    preferred = event.target instanceof Element ? event.target.closest<HTMLElement>(ANCHORS) : null;
    capture();
  };
  const mutation = new MutationObserver(restore);
  // Text and attribute churn (activity dots, statuses) never moves rows; size changes reach the ResizeObserver.
  mutation.observe(body, { childList: true, subtree: true, attributes: true, attributeFilter: ["data-dnd-dragging"] });
  const resize = new ResizeObserver(restore);
  resize.observe(body);
  resize.observe(viewport);
  viewport.addEventListener("scroll", onScroll, { passive: true });
  viewport.addEventListener("pointerdown", onPress, true);
  viewport.addEventListener("keydown", onPress, true);
  capture();
  return () => {
    disposed = true;
    mutation.disconnect();
    resize.disconnect();
    viewport.removeEventListener("scroll", onScroll);
    viewport.removeEventListener("pointerdown", onPress, true);
    viewport.removeEventListener("keydown", onPress, true);
    space.style.height = "0px";
  };
}
