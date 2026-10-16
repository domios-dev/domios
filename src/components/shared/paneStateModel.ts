import { describeError, type ErrorAction, type ErrorContext, type ErrorDescription } from "../../lib/describeError";

export interface PaneStateContext extends ErrorContext {
  scope?: "tab" | "workspace";
  description?: ErrorDescription;
}

export interface PaneStateModel {
  icon: "folder-off" | "alert" | "sign-in";
  title: string;
  sentence: string;
  path: string | null;
  actions: ErrorAction[];
  details: string;
}

export function paneStateModel(error: unknown, context: PaneStateContext = {}): PaneStateModel {
  const description = context.description ?? describeError(error, context);
  const missingFolder = description.actions.includes("choose-folder");
  return {
    icon: missingFolder ? "folder-off" : description.actions.includes("sign-in") ? "sign-in" : "alert",
    title: missingFolder ? "Folder unavailable" : description.title,
    sentence: missingFolder
      ? context.scope === "workspace" ? "Choose a folder to reopen this workspace." : "Choose a folder to start this tab again."
      : description.cause,
    path: context.cwd ?? null,
    actions: description.actions,
    details: description.raw,
  };
}
