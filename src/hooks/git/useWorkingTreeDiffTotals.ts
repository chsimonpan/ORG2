/**
 * Shared working-tree numstat store.
 *
 * Several always-mounted workstation surfaces display the same `+N -N`
 * badge. Keeping the request and WebSocket subscription in each component
 * turned one `repo:status_updated` event into N identical HTTP requests.
 * This module owns one subscription and one in-flight request per repo path;
 * React consumers read the shared snapshot through `useSyncExternalStore`.
 */
import { useCallback, useSyncExternalStore } from "react";

import { getGitDiffNumstatCombined } from "@src/api/http/git/diff";
import { getCodeEditorWebSocket } from "@src/api/realtime/codeEditorWebSocket";

export interface WorkingTreeDiffTotals {
  additions: number;
  deletions: number;
}

export interface WorkingTreeNumstatSnapshot extends WorkingTreeDiffTotals {
  /** Raw repository paths, matching the Git API response. */
  files: ReadonlyMap<string, { additions: number; deletions: number }>;
}

const EMPTY_FILES: ReadonlyMap<
  string,
  { additions: number; deletions: number }
> = new Map();
const EMPTY_SNAPSHOT: WorkingTreeNumstatSnapshot = {
  additions: 0,
  deletions: 0,
  files: EMPTY_FILES,
};
const REFRESH_DEBOUNCE_MS = 300;

interface NumstatEntry {
  key: string;
  repoId: string;
  repoPath: string;
  snapshot: WorkingTreeNumstatSnapshot;
  subscribers: Set<() => void>;
  unsubscribeStatus?: () => void;
  debounceTimer?: ReturnType<typeof setTimeout>;
  inFlight?: Promise<void>;
  refreshAfterInFlight: boolean;
  refreshOnVisible: boolean;
  visibilityHandler?: () => void;
}

const entriesByKey = new Map<string, NumstatEntry>();

function repoKeyOf(repoId: string, repoPath: string): string {
  return `${repoId}\u0000${repoPath}`;
}

function notify(entry: NumstatEntry): void {
  for (const subscriber of entry.subscribers) subscriber();
}

function snapshotsEqual(
  left: WorkingTreeNumstatSnapshot,
  right: WorkingTreeNumstatSnapshot
): boolean {
  if (
    left.additions !== right.additions ||
    left.deletions !== right.deletions ||
    left.files.size !== right.files.size
  ) {
    return false;
  }
  for (const [path, stats] of left.files) {
    const candidate = right.files.get(path);
    if (
      !candidate ||
      candidate.additions !== stats.additions ||
      candidate.deletions !== stats.deletions
    ) {
      return false;
    }
  }
  return true;
}

function getOrCreateEntry(repoId: string, repoPath: string): NumstatEntry {
  const key = repoKeyOf(repoId, repoPath);
  const existing = entriesByKey.get(key);
  if (existing) return existing;

  const entry: NumstatEntry = {
    key,
    repoId,
    repoPath,
    snapshot: EMPTY_SNAPSHOT,
    subscribers: new Set(),
    refreshAfterInFlight: false,
    refreshOnVisible: false,
  };
  entriesByKey.set(key, entry);
  return entry;
}

function refreshEntry(entry: NumstatEntry): void {
  if (entry.inFlight) {
    entry.refreshAfterInFlight = true;
    return;
  }

  entry.inFlight = getGitDiffNumstatCombined({
    repo_id: entry.repoId,
    repo_path: entry.repoPath,
    include_untracked: true,
  })
    .then((result) => {
      if (!result) return;
      const next: WorkingTreeNumstatSnapshot = {
        additions: result.totalInsertions ?? 0,
        deletions: result.totalDeletions ?? 0,
        files: new Map(
          result.files.map((file) => [
            file.path,
            {
              additions: file.insertions ?? 0,
              deletions: file.deletions ?? 0,
            },
          ])
        ),
      };
      if (snapshotsEqual(entry.snapshot, next)) return;
      entry.snapshot = next;
      notify(entry);
    })
    .catch(() => {
      // Diff totals are cosmetic. Preserve the last good snapshot on failure.
    })
    .finally(() => {
      entry.inFlight = undefined;
      if (entry.refreshAfterInFlight && entry.subscribers.size > 0) {
        entry.refreshAfterInFlight = false;
        refreshEntry(entry);
      }
    });
}

function scheduleRefresh(entry: NumstatEntry): void {
  if (entry.debounceTimer) return;
  entry.debounceTimer = setTimeout(() => {
    entry.debounceTimer = undefined;
    refreshEntry(entry);
  }, REFRESH_DEBOUNCE_MS);
}

function startEntry(entry: NumstatEntry): void {
  if (!entry.unsubscribeStatus) {
    const websocket = getCodeEditorWebSocket();
    if (websocket) {
      entry.unsubscribeStatus = websocket.on("repo:status_updated", (data) => {
        const payload = data as { repo_id?: string };
        if (payload.repo_id !== entry.repoId) return;
        if (typeof document !== "undefined" && document.hidden) {
          entry.refreshOnVisible = true;
          return;
        }
        scheduleRefresh(entry);
      });
    }
  }
  if (typeof document !== "undefined" && !entry.visibilityHandler) {
    entry.visibilityHandler = () => {
      if (document.hidden || !entry.refreshOnVisible) return;
      entry.refreshOnVisible = false;
      scheduleRefresh(entry);
    };
    document.addEventListener("visibilitychange", entry.visibilityHandler);
  }
  refreshEntry(entry);
}

function stopEntry(entry: NumstatEntry): void {
  entry.unsubscribeStatus?.();
  entry.unsubscribeStatus = undefined;
  if (entry.debounceTimer) {
    clearTimeout(entry.debounceTimer);
    entry.debounceTimer = undefined;
  }
  if (typeof document !== "undefined" && entry.visibilityHandler) {
    document.removeEventListener("visibilitychange", entry.visibilityHandler);
    entry.visibilityHandler = undefined;
  }
  entry.refreshOnVisible = false;
}

export function subscribeWorkingTreeNumstat(
  repoId: string,
  repoPath: string,
  callback: () => void
): () => void {
  const entry = getOrCreateEntry(repoId, repoPath);
  entry.subscribers.add(callback);
  if (entry.subscribers.size === 1) startEntry(entry);

  return () => {
    entry.subscribers.delete(callback);
    if (entry.subscribers.size !== 0) return;
    stopEntry(entry);
    // React Strict Mode briefly unsubscribes and re-subscribes during one
    // commit. Defer deletion so that cycle retains the same shared entry.
    queueMicrotask(() => {
      if (
        entry.subscribers.size === 0 &&
        entriesByKey.get(entry.key) === entry
      ) {
        entriesByKey.delete(entry.key);
      }
    });
  };
}

export function getWorkingTreeNumstatSnapshot(
  repoId: string,
  repoPath: string
): WorkingTreeNumstatSnapshot {
  return (
    entriesByKey.get(repoKeyOf(repoId, repoPath))?.snapshot ?? EMPTY_SNAPSHOT
  );
}

/**
 * Returns one shared per-file numstat snapshot for a repository. Consumers
 * with the same repo id/path use one WebSocket subscription and one request.
 */
export function useWorkingTreeNumstat(
  repoId: string | undefined,
  repoPath: string | undefined
): WorkingTreeNumstatSnapshot {
  const active = Boolean(repoId && repoPath);
  const subscribe = useCallback(
    (callback: () => void) => {
      if (!active || !repoId || !repoPath) return () => {};
      return subscribeWorkingTreeNumstat(repoId, repoPath, callback);
    },
    [active, repoId, repoPath]
  );
  const getSnapshot = useCallback(() => {
    if (!active || !repoId || !repoPath) return EMPTY_SNAPSHOT;
    return getWorkingTreeNumstatSnapshot(repoId, repoPath);
  }, [active, repoId, repoPath]);

  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}

/**
 * Fetches working-tree additions/deletions once per repo and shares the
 * result across the status bar, tab menu, start page, and focused-chat rail.
 */
export function useWorkingTreeDiffTotals(
  repoId: string | undefined,
  repoPath: string | undefined
): WorkingTreeDiffTotals {
  return useWorkingTreeNumstat(repoId, repoPath);
}
