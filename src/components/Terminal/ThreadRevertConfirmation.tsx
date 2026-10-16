import { ConfirmDialog } from "../Extensions/ConfirmDialog";

export function ThreadRevertConfirmation({
  turnId,
  onConfirm,
  onCancel,
}: {
  turnId: string;
  onConfirm: (turnId: string) => void;
  onCancel: () => void;
}) {
  return (
    <ConfirmDialog
      title="Revert conversation history?"
      message="The runtime will remove every turn after this one from this tab."
      confirmLabel="Revert"
      testId="thread-revert-confirm"
      onCancel={onCancel}
      onConfirm={() => onConfirm(turnId)}
    />
  );
}
