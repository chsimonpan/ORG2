import type {
  JourneyReview,
  JourneySnapshot,
} from "@src/api/tauri/sessionJourney";

export type ReviewPanelMode = "dock" | "float" | "hidden";
export const REVIEW_PANEL_STORAGE_KEY = "orgii-session-journey-review-panel";

export function activeTask(snapshot: JourneySnapshot | null) {
  return snapshot?.active_task_id
    ? (snapshot.tasks[snapshot.active_task_id] ?? null)
    : null;
}

export function visibleReviews(
  snapshot: JourneySnapshot | null
): JourneyReview[] {
  // The Journey is an audit trail: confirmed and discarded reviews remain
  // inspectable rather than disappearing from the Desktop surface.
  return Object.values(snapshot?.reviews ?? {});
}

export function isRevisionConflict(error: unknown): boolean {
  return String(error).includes("修订冲突");
}

export function hasRecoverableJourney(
  snapshot: JourneySnapshot | null
): boolean {
  if (!snapshot) return false;
  return (
    Boolean(snapshot.active_task_id) ||
    Object.values(snapshot.branches).some(
      (fork) =>
        fork.id !== fork.parent_branch_id &&
        (fork.state === "active" || fork.state === "closing")
    )
  );
}

export interface JourneyTreeEntry {
  id: string;
  kind: "task" | "fork";
  name: string;
  taskId: string;
  branchId: string;
  state: string;
  active: boolean;
  children: JourneyTreeEntry[];
}

/**
 * Durable editing tree for one Session. IDs come straight from the aggregate;
 * labels never manufacture a second identity. A fork owns the task created
 * with it, while main-branch tasks remain roots.
 */
export function accumulatedJourneyTree(
  snapshot: JourneySnapshot | null
): JourneyTreeEntry[] {
  if (!snapshot) return [];
  const tasks = Object.values(snapshot.tasks);
  const taskEntry = (task: (typeof tasks)[number]): JourneyTreeEntry => ({
    id: `task:${task.id}`,
    kind: "task",
    name: task.name,
    taskId: task.id,
    branchId: task.branch_id,
    state: task.state,
    active: snapshot.active_task_id === task.id,
    children: [],
  });
  const roots = tasks
    .filter((task) => task.branch_id === "main")
    .map(taskEntry);
  for (const branch of Object.values(snapshot.branches)) {
    if (branch.id === branch.parent_branch_id) continue;
    const branchTasks = tasks.filter((task) => task.branch_id === branch.id);
    const displayName =
      branch.name?.trim() || branchTasks[0]?.name || branch.id;
    roots.push({
      id: `fork:${branch.id}`,
      kind: "fork",
      name: displayName,
      taskId: branchTasks[0]?.id ?? "",
      branchId: branch.id,
      state: branch.state,
      active: snapshot.active_branch_id === branch.id,
      children: branchTasks.map(taskEntry),
    });
  }
  return roots;
}
