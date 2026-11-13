import { useLayoutEffect, useRef } from "react";

export function useTemplateViewFocus(view: string, template: string | undefined) {
  const body = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const content = body.current;
    if (!content) return;
    const target = view === "form"
      ? content.querySelector<HTMLElement>('[data-testid="template-start-form"] input:not([disabled])') ?? content.querySelector<HTMLElement>('h3[tabindex="-1"]')
      : content.querySelector<HTMLElement>('h2, h3, input[type="search"]');
    if (target) {
      if (!target.matches("input")) target.tabIndex = -1;
      target.focus();
    }
  }, [view, template]);
  return body;
}
