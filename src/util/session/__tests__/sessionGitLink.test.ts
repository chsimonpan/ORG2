import { describe, expect, it } from "vitest";

import { resolveSessionGitLink } from "@src/util/session/sessionGitLink";

describe("resolveSessionGitLink", () => {
  it("returns null when the session reports no branch at all", () => {
    expect(resolveSessionGitLink({})).toBeNull();
    expect(resolveSessionGitLink({ branch: "" })).toBeNull();
  });

  it("uses the source-reported branch for imported sessions", () => {
    expect(resolveSessionGitLink({ branch: "refs/heads/main" })).toEqual({
      branch: "main",
      isActiveWorktree: false,
    });
  });

  it("prefers the worktree branch over the launch branch", () => {
    expect(
      resolveSessionGitLink({
        branch: "main",
        worktreeBranch: "agent/abc123",
        worktreePath: "/tmp/wt",
      })
    ).toEqual({ branch: "abc123", isActiveWorktree: true });
  });

  it("falls back to the base branch when nothing else is recorded", () => {
    expect(resolveSessionGitLink({ baseBranch: "develop" })).toEqual({
      branch: "develop",
      isActiveWorktree: false,
    });
  });

  it("keeps a pending worktree active", () => {
    expect(
      resolveSessionGitLink({
        worktreeBranch: "agent/abc123",
        mergeStatus: "pending",
      })?.isActiveWorktree
    ).toBe(true);
  });

  it("settles a worktree once it is merged or conflicted", () => {
    // A settled worktree is no longer work in flight, so the sidebar stops
    // marking it — only a real PR can speak for it after that.
    expect(
      resolveSessionGitLink({
        worktreeBranch: "agent/abc123",
        mergeStatus: "merged",
      })?.isActiveWorktree
    ).toBe(false);
    expect(
      resolveSessionGitLink({
        worktreeBranch: "agent/abc123",
        mergeStatus: "conflict",
      })?.isActiveWorktree
    ).toBe(false);
  });
});
