import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { composerIssues, type ComposerAttachment, type ComposerIssue, type ComposerIssueAction } from "./composerIssues";
import type { AttachmentUpload } from "./attachmentUploadModel";

export interface ActionableComposerIssue extends ComposerIssue { run: (action: ComposerIssueAction) => void }
const ComposerIssuesContext = createContext({
  provided: false,
  issues: [] as ActionableComposerIssue[],
  publish: (_owner: object, _issues: ActionableComposerIssue[]) => {},
});

export function ComposerIssuesProvider({ children }: { children: ReactNode }) {
  const [owners, setOwners] = useState(new Map<object, ActionableComposerIssue[]>());
  const publish = useCallback((owner: object, issues: ActionableComposerIssue[]) => {
    setOwners(previous => {
      if (!issues.length && !previous.has(owner)) return previous;
      const next = new Map(previous);
      if (issues.length) next.set(owner, issues); else next.delete(owner);
      return next;
    });
  }, []);
  return <ComposerIssuesContext.Provider value={{ provided: true, issues: [...owners.values()].flat(), publish }}>{children}</ComposerIssuesContext.Provider>;
}

export const useComposerIssues = () => useContext(ComposerIssuesContext).issues;
export const useHasComposerIssuesProvider = () => useContext(ComposerIssuesContext).provided;

export function usePublishComposerIssues(uploads: readonly AttachmentUpload[], handlers: {
  retry: (id: number) => unknown;
  remove: (id: number) => unknown;
  sendPath?: (id: number) => unknown;
}, attachments: readonly ComposerAttachment[] = []) {
  const { publish } = useContext(ComposerIssuesContext);
  const owner = useRef({}).current;
  const latest = useRef(handlers);
  latest.current = handlers;
  const paths = JSON.stringify(attachments);
  useEffect(() => {
    const issues = composerIssues(JSON.parse(paths) as ComposerAttachment[], uploads);
    publish(owner, issues.map(issue => ({ ...issue, run: action => {
      if (action === "retry") void latest.current.retry(issue.id);
      else if (action === "send-path") void latest.current.sendPath?.(issue.id);
      else void latest.current.remove(issue.id);
    } })));
  }, [uploads, paths, owner, publish]);
  useEffect(() => () => publish(owner, []), [owner, publish]);
}
