import { shellFileOperations, toolCommand, type ToolEvent } from "@codeterm/chat-core";

export function shellFileOperationPreview(event: ToolEvent) {
  if ((event.kind !== "command" && event.kind !== "codeterm") || event.fileDiffs?.length || event.edits?.length || event.commandBatch?.length) return null;
  const input = event.message.toolInput;
  const hasCommand = [input?.command, input?.cmd, event.detail].some(value => typeof value === "string" && value.trim().length > 0);
  if (!hasCommand) return null;
  const { command, argv } = toolCommand(event);
  const analysis = shellFileOperations(command, argv);
  if (analysis.operations.length === 0) return null;
  return (
    <ul aria-label="Intended file operations" className="m-0 list-none px-3 py-0.5 font-mono text-[11.5px]" style={{ color: "var(--ct-muted-fg)" }}>
      {analysis.operations.map((operation, index) => (
        <li key={index} className="min-w-0 truncate" title={operation.path ?? undefined}>
          {operation.action === "append" ? "Intended append" : "Intended write"}{": "}{operation.path ?? "unresolved target"}
        </li>
      ))}
      {analysis.limited && <li>Preview limit reached</li>}
    </ul>
  );
}
