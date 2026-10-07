// @vitest-environment jsdom
import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import type { Session } from "@src/store/session/sessionAtom/types";

import SessionForkHeaderExtras from "./index";

const mocks = vi.hoisted(() => ({
  fork: vi.fn(),
  open: vi.fn(),
  replace: vi.fn(),
  error: vi.fn(),
}));
vi.mock("jotai", () => ({
  useAtomValue: () => [],
  useSetAtom: () => mocks.replace,
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (s: string) => s }),
}));
vi.mock("@src/components/Button", () => ({
  default: ({
    children,
    loading,
    iconOnly,
    layout,
    variant,
    size,
    icon,
    ...props
  }: any) => React.createElement("button", props, children),
}));
vi.mock("@src/components/Tooltip", () => ({
  default: ({ children }: any) => children,
}));
vi.mock("@src/components/Tag", () => ({
  default: ({ children }: any) => children,
}));
vi.mock("@src/components/Message", () => ({
  default: { error: mocks.error, info: vi.fn() },
}));
vi.mock("@src/icons", () => ({ GitForkIcon: {}, HugeiconsIcon: () => null }));
vi.mock("@src/features/Org2Cloud/org2CloudRemoteSessionsAtom", () => ({
  org2CloudRemoteSessionsAtom: {},
}));
vi.mock("@src/features/Org2Cloud/useCloudSessionActions", () => ({
  useCloudSessionActions: () => ({ replaySession: vi.fn() }),
}));
vi.mock("@src/hooks/ui/tabs/useSessionView", () => ({
  useSessionView: () => ({ openSession: mocks.open }),
}));
vi.mock("@src/store/chatPanel/chatPanelTabsAtom", () => ({
  openOrReplaceSessionInChatPanelTabAtom: {},
}));
vi.mock("@src/store/session/sessionAtom/atoms", () => ({ sessionsAtom: {} }));
vi.mock("../../forkSession", () => ({ getSessionForkedFrom: () => undefined }));
vi.mock("../../useForkImportedSession", () => ({
  useForkImportedSession: () => ({ fork: mocks.fork, state: "idle" }),
}));
let root: ReturnType<typeof createRoot>;
let host: HTMLDivElement;
function session(id: string) {
  return {
    session_id: id,
    importedFrom: { shareToken: "synthetic-test-share" },
  } as Session;
}
async function render(id: string) {
  await act(async () =>
    root.render(
      React.createElement(SessionForkHeaderExtras, { session: session(id) })
    )
  );
}
function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}
beforeEach(() => {
  (globalThis as any).IS_REACT_ACT_ENVIRONMENT = true;
  vi.clearAllMocks();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});
it("late A fork never replaces the user's now-active B tab", async () => {
  const request = deferred<any>();
  mocks.fork.mockReturnValueOnce(request.promise);
  await render("A");
  await act(async () =>
    host.querySelector<HTMLButtonElement>("button")!.click()
  );
  await render("B");
  await act(async () =>
    request.resolve({ ok: true, localSessionId: "A-fork", name: "fork A" })
  );
  expect(mocks.replace).not.toHaveBeenCalled();
  expect(mocks.open).not.toHaveBeenCalled();
});
it("A→B→A revision rejects the first A fork but opens the new A fork", async () => {
  const old = deferred<any>(),
    current = deferred<any>();
  mocks.fork
    .mockReturnValueOnce(old.promise)
    .mockReturnValueOnce(current.promise);
  await render("A");
  await act(async () =>
    host.querySelector<HTMLButtonElement>("button")!.click()
  );
  await render("B");
  await render("A");
  await act(async () =>
    host.querySelector<HTMLButtonElement>("button")!.click()
  );
  await act(async () =>
    old.resolve({ ok: true, localSessionId: "old-A-fork", name: "old" })
  );
  expect(mocks.replace).not.toHaveBeenCalled();
  await act(async () =>
    current.resolve({ ok: true, localSessionId: "new-A-fork", name: "new" })
  );
  expect(mocks.replace).toHaveBeenCalledExactlyOnceWith({
    sessionId: "new-A-fork",
    sessionName: "new",
    repoPath: undefined,
  });
  expect(mocks.open).toHaveBeenCalledExactlyOnceWith(
    "new-A-fork",
    "new",
    undefined
  );
});
it("late fork failure does not toast over the unrelated B session", async () => {
  const request = deferred<any>();
  mocks.fork.mockReturnValueOnce(request.promise);
  await render("A");
  await act(async () =>
    host.querySelector<HTMLButtonElement>("button")!.click()
  );
  await render("B");
  await act(async () => request.resolve({ ok: false, errorKind: "generic" }));
  expect(mocks.error).not.toHaveBeenCalled();
});
it("unmounted fork does not navigate or replace any tab", async () => {
  const request = deferred<any>();
  mocks.fork.mockReturnValueOnce(request.promise);
  await render("A");
  await act(async () =>
    host.querySelector<HTMLButtonElement>("button")!.click()
  );
  await act(async () => root.unmount());
  await act(async () =>
    request.resolve({ ok: true, localSessionId: "A-fork", name: "fork" })
  );
  expect(mocks.open).not.toHaveBeenCalled();
});
