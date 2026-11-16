export function receiverLocation(value: unknown): { installationId: string | null; instanceId: string | null; online: boolean | null } {
  if (typeof value !== "object" || value === null) return { installationId: null, instanceId: null, online: null };
  const installationId = "receiver_installation_id" in value && typeof value.receiver_installation_id === "string" ? value.receiver_installation_id : null;
  const instanceId = "receiver_instance_id" in value && typeof value.receiver_instance_id === "string" ? value.receiver_instance_id : null;
  const online = "receiver_online" in value && typeof value.receiver_online === "boolean" ? value.receiver_online : null;
  return { installationId, instanceId, online };
}

export function receiverLocationFields(value: unknown) {
  const location = receiverLocation(value);
  return {
    ...(location.installationId !== null ? { receiver_installation_id: location.installationId } : {}),
    ...(location.instanceId !== null ? { receiver_instance_id: location.instanceId } : {}),
    ...(location.online !== null ? { receiver_online: location.online } : {}),
  };
}

