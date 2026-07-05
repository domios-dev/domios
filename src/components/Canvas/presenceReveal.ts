import { atLeast, type World } from "./worlds";

export function shouldRevealPresenceDetails(
  world: World,
  taskId: string,
  emphasisTaskIds: ReadonlySet<string>,
  pinHovered: boolean,
): boolean {
  return pinHovered || emphasisTaskIds.has(taskId) || atLeast(world, "W2");
}
