/**
 * Remote-URL parsing shared by PR creation and the sidebar's PR status cache.
 */

/**
 * `git@host:owner/repo.git` / `https://host/owner/repo.git` → `owner/repo`.
 * Host-agnostic and unvalidated: callers that hit the GitHub API must go
 * through {@link resolveGithubRepoFullName} instead.
 */
export function parseRepoFullNameFromRemote(remoteUrl: string): string | null {
  const sshMatch = remoteUrl.match(/git@[^:]+:(.+?)(?:\.git)?$/);
  if (sshMatch) return sshMatch[1] ?? null;
  const httpsMatch = remoteUrl.match(/https?:\/\/[^/]+\/(.+?)(?:\.git)?$/);
  if (httpsMatch) return httpsMatch[1] ?? null;
  return null;
}

/** github.com only — GitHub Enterprise hosts have their own API base. */
function isGithubRemote(remoteUrl: string): boolean {
  return /(^|@|\/\/)github\.com([:/]|$)/i.test(remoteUrl);
}

/**
 * First github.com remote in `remotes`, as a strict `owner/repo`.
 *
 * Rejects anything that does not parse to exactly two path segments so a
 * stray blob/tree URL cached as a "remote" can never be pasted into an API
 * path.
 */
export function resolveGithubRepoFullName(
  remotes: readonly string[] | undefined
): string | null {
  if (!remotes) return null;
  for (const remote of remotes) {
    if (!remote || !isGithubRemote(remote)) continue;
    const fullName = parseRepoFullNameFromRemote(remote);
    if (fullName && /^[^/\s]+\/[^/\s]+$/.test(fullName)) return fullName;
  }
  return null;
}
