/**
 * Resolve the Git linkage a session row advertises in the sidebar.
 *
 * Read-only over metadata the session already carries: local sessions from
 * their launch params (`worktreeBranch` / `branch` / `baseBranch` /
 * `mergeStatus`), imported sessions from whatever branch their SOURCE app
 * recorded (Claude Code transcripts, Cursor/Windsurf tracked-repo metadata).
 * Nothing here inspects a working copy or shells out to git — a session whose
 * source never reported a branch simply has no link.
 */
import { formatBranchLabel } from "@src/util/git/branchLabel";

export interface SessionGitLink {
  /** Branch name with `refs/heads/` and the `agent/` worktree prefix stripped. */
  branch: string;
  /**
   * The branch lives in an isolated worktree that has not yet been merged or
   * conflicted out — i.e. work still in flight. This is the only branch-level
   * state the sidebar marks on its own; everything else it shows comes from a
   * real pull request.
   */
  isActiveWorktree: boolean;
}

export interface SessionGitLinkSource {
  branch?: string;
  worktreePath?: string;
  worktreeBranch?: string;
  baseBranch?: string;
  mergeStatus?: string;
}

/**
 * Branch precedence mirrors `SessionHoverCardContent`: the worktree branch is
 * where the session actually ran, so it outranks the repo branch captured at
 * launch, which in turn outranks the base branch a worktree forked from.
 */
export function resolveSessionGitLink(
  session: SessionGitLinkSource
): SessionGitLink | null {
  const worktreeBranch = formatBranchLabel(session.worktreeBranch);
  const branch =
    worktreeBranch ||
    formatBranchLabel(session.branch) ||
    formatBranchLabel(session.baseBranch);
  if (!branch) return null;

  const isWorktree = Boolean(worktreeBranch || session.worktreePath);
  const isSettled =
    session.mergeStatus === "merged" || session.mergeStatus === "conflict";

  return { branch, isActiveWorktree: isWorktree && !isSettled };
}
