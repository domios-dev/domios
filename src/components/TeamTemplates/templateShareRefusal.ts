export interface ShareRefusal {
  message: string;
  action?: "sign_in" | "refresh";
}

export function templateShareRefusal(reason?: string): ShareRefusal {
  switch (reason) {
    case "installation_unverified": return { message: "This machine needs to sign in again before it can share.", action: "sign_in" };
    case "entitlements_unavailable": return { message: "We couldn't confirm your plan right now. Try again.", action: "refresh" };
    case "friends_locked": return { message: "Your Neighbors channel is locked. Upgrade in Settings > Account to share." };
    case "recipient_locked": return { message: "This Neighbor's channel is locked. Ask them to check their plan before sharing." };
    case "recipient_not_receiving": return { message: "This Neighbor is not receiving messages. Ask them to enable receiving before sharing." };
    case "recipient_paused": return { message: "This Neighbor has paused messages from you. Ask them to resume before sharing." };
    case "neighbors_disabled": return { message: "Your Neighbors channel is off. Turn Neighbors on before sharing." };
    default: return { message: "The template could not be shared. Check your Neighbors conversation and try again later." };
  }
}
