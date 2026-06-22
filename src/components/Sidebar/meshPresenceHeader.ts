export type MeshPresence = "online" | "offline";

export interface MeshPresenceSummary {
  label: string;
  title: string;
}

export interface MeshPresenceMenuItem {
  value: MeshPresence;
  label: string;
  sub: string;
  active: boolean;
}

export function getMeshPresenceSummary(state: MeshPresence): MeshPresenceSummary {
  if (state === "online") {
    return {
      label: "Online",
      title: "Mesh: online — visible to peers",
    };
  }
  return {
    label: "Offline",
    title: "Mesh: offline",
  };
}

export function getMeshPresenceMenuItems(state: MeshPresence): MeshPresenceMenuItem[] {
  return [
    {
      value: "online",
      label: "Online",
      sub: "Visible to peers",
      active: state === "online",
    },
    {
      value: "offline",
      label: "Offline",
      sub: "Invisible to peers",
      active: state === "offline",
    },
  ];
}
