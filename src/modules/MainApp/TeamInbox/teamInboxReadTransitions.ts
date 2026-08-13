import type { TeamInboxDataSource, TeamInboxItem } from "./domain";

export type TeamInboxReadTransitionKind = "read" | "unread";

export interface TeamInboxReadTransitionResult {
  ok: boolean;
  /**
   * Only meaningful when `ok` is false. `true` means a resync against the
   * data source's server-truth state was kicked off after the failed
   * optimistic mutation (regardless of whether that resync itself
   * succeeded) so the caller knows a follow-up render is coming.
   */
  resyncStarted: boolean;
}

/**
 * Drives a single Team Inbox mark-read / mark-unread mutation.
 *
 * The data source (`teamInboxCoordinator`) already reverts its own
 * optimistic item mutation when the underlying local/cloud call rejects.
 * This helper adds the second half of that contract: it also triggers a
 * `refresh()` on failure so the list and unread badge resync against
 * server truth instead of relying solely on the coordinator's local
 * revert, which guards against drift if the revert races with another
 * concurrent update (e.g. a mutation from another client). It is kept
 * framework-free (no React) so it can be unit tested directly — repo
 * policy is not to unit test `.tsx` files.
 */
export async function performTeamInboxReadTransition(
  kind: TeamInboxReadTransitionKind,
  item: TeamInboxItem,
  dataSource: Pick<TeamInboxDataSource, "markRead" | "markUnread" | "refresh">
): Promise<TeamInboxReadTransitionResult> {
  const mutate = kind === "read" ? dataSource.markRead : dataSource.markUnread;
  if (!mutate) return { ok: true, resyncStarted: false };

  try {
    await mutate(item);
    return { ok: true, resyncStarted: false };
  } catch {
    if (!dataSource.refresh) return { ok: false, resyncStarted: false };
    // Best-effort: the resync's own outcome doesn't change the fact that
    // the mutation itself failed and the caller still needs to surface an
    // error — it only affects whether a follow-up render is coming.
    try {
      await dataSource.refresh();
    } catch {
      // Swallow: the refresh failure is secondary to the mutation failure
      // already being reported via `ok: false`.
    }
    return { ok: false, resyncStarted: true };
  }
}
