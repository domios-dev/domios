export function UnavailableNotice({ text }: { text: string }) {
  const [status, ...instruction] = text.split("\n");
  return <><span className="bp-unavailable-status">{status}</span><span className="bp-unavailable-instruction">{instruction.join("\n")}</span></>;
}
