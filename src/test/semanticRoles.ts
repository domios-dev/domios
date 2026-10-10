export function accessibleName(element: Element): string {
  const ids = element.getAttribute("aria-labelledby");
  if (ids) return ids.split(" ").map((id) => element.ownerDocument.getElementById(id)?.textContent ?? "").join(" ").trim();
  return (element.getAttribute("aria-label") ?? element.textContent ?? "").trim();
}

export function getByRole(container: ParentNode, role: "button" | "dialog", name: string): HTMLElement {
  const selector = role === "button" ? 'button, [role="button"]' : '[role="dialog"]';
  const element = Array.from(container.querySelectorAll<HTMLElement>(selector)).find((candidate) => accessibleName(candidate) === name);
  if (!element) throw new Error(`No ${role} named ${name}`);
  return element;
}
