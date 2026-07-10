export const NAV_STACK_MAX = 20;

export type NavSurface = "task" | "plan" | "spec";

export interface NavEntry {
  surface: NavSurface;
  id: string;
  planId: string;
  scroll?: number;
}

export type NavStack = readonly NavEntry[];

export const EMPTY_NAV_STACK: NavStack = [];

export function navPush(stack: NavStack, entry: NavEntry): NavStack {
  return [...stack, entry].slice(-NAV_STACK_MAX);
}

export function navPop(stack: NavStack): { stack: NavStack; entry: NavEntry | undefined } {
  if (stack.length === 0) return { stack, entry: undefined };
  return {
    stack: stack.slice(0, -1),
    entry: stack[stack.length - 1],
  };
}

export function navCanGoBack(stack: NavStack): boolean {
  return stack.length > 0;
}
