import { afterEach, describe, expect, it, vi } from "vitest";

import {
  getWorkingTreeNumstatSnapshot,
  subscribeWorkingTreeNumstat,
} from "./useWorkingTreeDiffTotals";

const mocks = vi.hoisted(() => ({
  getGitDiffNumstatCombined: vi.fn(),
  on: vi.fn(),
  unsubscribe: vi.fn(),
}));

vi.mock("@src/api/http/git/diff", () => ({
  getGitDiffNumstatCombined: mocks.getGitDiffNumstatCombined,
}));

vi.mock("@src/api/realtime/codeEditorWebSocket", () => ({
  getCodeEditorWebSocket: () => ({ on: mocks.on }),
}));

async function flushPromises(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}

describe("working-tree numstat store", () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.clearAllMocks();
  });

  it("shares one listener and request across consumers of the same repo", async () => {
    vi.useFakeTimers();
    let statusHandler: ((data: { repo_id?: string }) => void) | undefined;
    mocks.on.mockImplementation(
      (_event: string, handler: (data: { repo_id?: string }) => void) => {
        statusHandler = handler;
        return mocks.unsubscribe;
      }
    );
    mocks.getGitDiffNumstatCombined
      .mockResolvedValueOnce({
        files: [
          {
            path: "src/example.ts",
            status: "M",
            insertions: 4,
            deletions: 2,
            binary: false,
          },
        ],
        totalInsertions: 4,
        totalDeletions: 2,
        filesChanged: 1,
      })
      .mockResolvedValueOnce({
        files: [],
        totalInsertions: 0,
        totalDeletions: 0,
        filesChanged: 0,
      });

    const firstConsumer = vi.fn();
    const secondConsumer = vi.fn();
    const unsubscribeFirst = subscribeWorkingTreeNumstat(
      "repo-1",
      "/workspace/repo",
      firstConsumer
    );
    const unsubscribeSecond = subscribeWorkingTreeNumstat(
      "repo-1",
      "/workspace/repo",
      secondConsumer
    );

    expect(mocks.on).toHaveBeenCalledTimes(1);
    expect(mocks.on).toHaveBeenCalledWith(
      "repo:status_updated",
      expect.any(Function)
    );
    expect(mocks.getGitDiffNumstatCombined).toHaveBeenCalledTimes(1);

    await flushPromises();
    expect(firstConsumer).toHaveBeenCalledTimes(1);
    expect(secondConsumer).toHaveBeenCalledTimes(1);
    expect(
      getWorkingTreeNumstatSnapshot("repo-1", "/workspace/repo")
    ).toMatchObject({ additions: 4, deletions: 2 });

    statusHandler?.({ repo_id: "another-repo" });
    await vi.advanceTimersByTimeAsync(300);
    expect(mocks.getGitDiffNumstatCombined).toHaveBeenCalledTimes(1);

    statusHandler?.({ repo_id: "repo-1" });
    await vi.advanceTimersByTimeAsync(300);
    expect(mocks.getGitDiffNumstatCombined).toHaveBeenCalledTimes(2);

    unsubscribeFirst();
    expect(mocks.unsubscribe).not.toHaveBeenCalled();
    unsubscribeSecond();
    expect(mocks.unsubscribe).toHaveBeenCalledTimes(1);
  });

  it("runs one follow-up when a refresh arrives during an in-flight request", async () => {
    vi.useFakeTimers();
    let statusHandler: ((data: { repo_id?: string }) => void) | undefined;
    mocks.on.mockImplementation(
      (_event: string, handler: (data: { repo_id?: string }) => void) => {
        statusHandler = handler;
        return mocks.unsubscribe;
      }
    );
    const first = deferred<{
      files: never[];
      totalInsertions: number;
      totalDeletions: number;
      filesChanged: number;
    }>();
    mocks.getGitDiffNumstatCombined
      .mockReturnValueOnce(first.promise)
      .mockResolvedValueOnce({
        files: [],
        totalInsertions: 2,
        totalDeletions: 1,
        filesChanged: 1,
      });

    const unsubscribe = subscribeWorkingTreeNumstat(
      "repo-race",
      "/workspace/race",
      vi.fn()
    );
    statusHandler?.({ repo_id: "repo-race" });
    await vi.advanceTimersByTimeAsync(300);
    expect(mocks.getGitDiffNumstatCombined).toHaveBeenCalledTimes(1);

    first.resolve({
      files: [],
      totalInsertions: 0,
      totalDeletions: 0,
      filesChanged: 0,
    });
    await flushPromises();

    expect(mocks.getGitDiffNumstatCombined).toHaveBeenCalledTimes(2);
    unsubscribe();
  });
});
