export function retainedSendReceipt(exitCode, stdout) {
  if (exitCode !== 1) return null;
  let value;
  try { value = JSON.parse(stdout); } catch { return null; }
  const receipt = value?.data ?? value;
  return receipt?.ok === true && receipt.delivery === "pending" &&
    receipt.bytes_written === false && typeof receipt.send_id === "string" && receipt.send_id.length > 0
    ? receipt : null;
}
