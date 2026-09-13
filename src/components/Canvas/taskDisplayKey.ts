const TASK_ID_PREFIX = "task_";

export interface TaskIdentity {
  id: string;
  key?: string | null;
}

/** The task's human key; tasks created before keys existed show their id, as the daemon's fallback does. */
export function taskDisplayKey(task: TaskIdentity): string {
  const key = task.key?.trim();
  if (key) return key;
  const bare = task.id.startsWith(TASK_ID_PREFIX) ? task.id.slice(TASK_ID_PREFIX.length) : task.id;
  return bare.toUpperCase();
}

/** Exact id first; a key is matched case-folded, as the daemon accepts it typed in any case. */
export function findTaskByReference<T extends TaskIdentity>(tasks: readonly T[], reference: string): T | undefined {
  const exact = tasks.find((task) => task.id === reference);
  if (exact) return exact;
  const folded = reference.trim().toUpperCase();
  return folded ? tasks.find((task) => taskDisplayKey(task).toUpperCase() === folded) : undefined;
}

export function taskReferenceLabel(task: TaskIdentity & { name: string }): string {
  const name = task.name.trim();
  const key = taskDisplayKey(task);
  return name ? `${key} ${name}` : key;
}

export function taskSegmentLabel(task: TaskIdentity): string {
  return `Task ${taskDisplayKey(task)}`;
}
