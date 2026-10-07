// @vitest-environment jsdom
import React, { act, useLayoutEffect } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import type { Session } from "@src/store/session/sessionAtom/types";

import { useForkImportedSession } from "./useForkImportedSession";

const mocks = vi.hoisted(() => ({
  fork: vi.fn(),
  fresh: vi.fn(),
  commit: vi.fn(() => true),
  auth: {},
}));
vi.mock("jotai", () => ({
  useAtom: () => [mocks.auth, vi.fn()],
  useAtomValue: () => [{ orgId: "synthetic-org" }],
}));
vi.mock("@src/hooks/logger", () => ({
  createLogger: () => ({ error: vi.fn(), warn: vi.fn() }),
}));
vi.mock("../Org2Cloud/org2CloudAuthAtom", () => ({
  org2CloudAuthAtom: {},
  commitRefreshedAuth: mocks.commit,
}));
vi.mock("../Org2Cloud/org2CloudOrgsAtom", () => ({ org2CloudOrgsAtom: {} }));
vi.mock("../Org2Cloud/org2CloudBackendAdapter", () => ({
  buildCloudSessionFetchClient: vi.fn(),
}));
vi.mock("../Org2Cloud/org2CloudClient", () => ({
  ensureFreshSession: mocks.fresh,
}));
vi.mock("../Org2Cloud/org2CloudShareEndpoint", () => ({
  resolvePersistedCloudShareEndpoint: vi.fn(),
}));
vi.mock("../Org2Cloud/org2CloudSharesClient", () => ({
  isOrg2ShareErrorCode: () => false,
  resolveCloudSessionShare: vi.fn(),
}));
vi.mock("../Org2Cloud/org2CloudSyncClient", () => ({
  isOrg2SyncErrorCode: () => false,
}));
vi.mock("./cloudSessionFork", () => ({
  executeAuthenticatedCloudSessionFork: mocks.fork,
}));
vi.mock("./forkSession", () => ({
  ForkCancelledError: class extends Error {},
  forkTeammateSession: vi.fn(),
}));
vi.mock("./forkSnapshotIntegrity", () => ({
  classifyForkOperationError: () => null,
}));
function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}
const sessions = Object.fromEntries(
  ["A", "B"].map((id) => [
    id,
    {
      session_id: id,
      importedFrom: { orgId: "synthetic-org", sourceSessionId: id },
    } as Session,
  ])
);
let root: ReturnType<typeof createRoot>,
  host: HTMLDivElement,
  value: ReturnType<typeof useForkImportedSession>;
function Harness({ id }: { id: string }) {
  const result = useForkImportedSession(sessions[id]);
  useLayoutEffect(() => {
    value = result;
  });
  return null;
}
async function render(id: string) {
  await act(async () => root.render(React.createElement(Harness, { id })));
}
beforeEach(() => {
  (globalThis as any).IS_REACT_ACT_ENVIRONMENT = true;
  vi.clearAllMocks();
  mocks.fresh.mockResolvedValue({ accessToken: "synthetic" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});
it("late A backend result leaves active B fork state untouched", async () => {
  const old = deferred<any>(),
    current = deferred<any>();
  mocks.fork
    .mockReturnValueOnce(old.promise)
    .mockReturnValueOnce(current.promise);
  await render("A");
  let oldResult!: ReturnType<typeof value.fork>;
  await act(async () => {
    oldResult = value.fork();
  });
  await render("B");
  expect(value.state).toBe("idle");
  let currentResult!: ReturnType<typeof value.fork>;
  await act(async () => {
    currentResult = value.fork();
  });
  expect(value.state).toBe("forking");
  await act(async () =>
    old.resolve({
      status: "ok",
      result: { localSessionId: "old-A", name: "old" },
    })
  );
  expect(await oldResult).toEqual({ ok: false, errorKind: "cancelled" });
  expect(value.state).toBe("forking");
  await act(async () =>
    current.resolve({
      status: "ok",
      result: { localSessionId: "B-fork", name: "B" },
    })
  );
  expect((await currentResult).ok).toBe(true);
  expect(value.state).toBe("idle");
});
it("A→B→A rejects stale auth before committing or starting the wrong source fork", async () => {
  const old = deferred<any>();
  mocks.fresh.mockReturnValueOnce(old.promise);
  await render("A");
  let result!: ReturnType<typeof value.fork>;
  await act(async () => {
    result = value.fork();
  });
  await render("B");
  await render("A");
  await act(async () => old.resolve({ accessToken: "late" }));
  expect(await result).toEqual({ ok: false, errorKind: "cancelled" });
  expect(mocks.commit).not.toHaveBeenCalled();
  expect(mocks.fork).not.toHaveBeenCalled();
  expect(value.state).toBe("idle");
});
it("old same-session request cannot replace the later request's state", async () => {
  const old = deferred<any>(),
    current = deferred<any>();
  mocks.fork
    .mockReturnValueOnce(old.promise)
    .mockReturnValueOnce(current.promise);
  await render("A");
  let oldResult!: ReturnType<typeof value.fork>;
  await act(async () => {
    oldResult = value.fork();
  });
  let currentResult!: ReturnType<typeof value.fork>;
  await act(async () => {
    currentResult = value.fork();
  });
  await act(async () => old.resolve({ status: "gone" }));
  expect(await oldResult).toEqual({ ok: false, errorKind: "cancelled" });
  expect(value.state).toBe("forking");
  expect(value.errorKind).toBeNull();
  await act(async () =>
    current.resolve({
      status: "ok",
      result: { localSessionId: "new-A", name: "new" },
    })
  );
  expect((await currentResult).ok).toBe(true);
  expect(value.state).toBe("idle");
});

it("a retained A callback invoked after switching B cannot start an A fork", async () => {
  await render("A");
  const oldCallback = value.fork;
  await render("B");
  let result!: Awaited<ReturnType<typeof oldCallback>>;
  await act(async () => {
    result = await oldCallback();
  });
  expect(result).toEqual({ ok: false, errorKind: "cancelled" });
  expect(mocks.fresh).not.toHaveBeenCalled();
  expect(mocks.fork).not.toHaveBeenCalled();
  expect(value.state).toBe("idle");
});
