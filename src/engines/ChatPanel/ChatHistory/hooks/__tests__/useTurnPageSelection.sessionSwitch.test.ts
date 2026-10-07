// @vitest-environment jsdom
import React, { act, useLayoutEffect } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import {
  useTurnPageNavigation,
  useTurnPageSelectionState,
} from "../useTurnPageSelection";

const mocks = vi.hoisted(() => ({ load: vi.fn(), prune: vi.fn(), epoch: 0 }));
vi.mock("jotai", () => ({ useAtomValue: () => mocks.epoch }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (s: string) => s }),
}));
vi.mock("@src/engines/SessionCore/core/atoms/metadata", () => ({
  transcriptReplaceEpochAtom: {},
}));
vi.mock("@src/engines/SessionCore/turns", () => ({
  loadSessionTurnBodyIntoStore: mocks.load,
  pruneLoadedTurnBodies: mocks.prune,
}));
vi.mock("@src/hooks/logger", () => ({
  createLogger: () => ({ warn: vi.fn() }),
}));
vi.mock("@src/util/session/sessionDispatch", () => ({
  isCollaborationImportedSession: () => false,
  isCodexAppSession: () => false,
}));
let host: HTMLDivElement,
  root: ReturnType<typeof createRoot>,
  value: ReturnType<typeof useTurnPageSelectionState>;
const pages = [
  {
    startGroupIndex: 0,
    endGroupIndex: 0,
    flatStartIndex: 0,
    flatEndIndex: 0,
    cursorIdeSummary: null,
  },
];
const groupMeta = [{ unloadedTurn: { turnId: "same-ordinal-turn" } }] as any;
function Harness({ id }: { id: string }) {
  const selected = useTurnPageSelectionState(id);
  useLayoutEffect(() => {
    value = selected;
  });
  useTurnPageNavigation({
    activeId: id,
    pageCount: 1,
    currentPageIndex: 0,
    pages,
    groupMeta,
    sessionLoadStatus: "loaded",
    turnPaginationEnabled: true,
    setTurnPageSelection: selected.setTurnPageSelection,
    setTurnPageListOpen: selected.setTurnPageListOpen,
  });
  return null;
}
async function render(id: string) {
  await act(async () => root.render(React.createElement(Harness, { id })));
}
function deferred() {
  let resolve!: () => void;
  let reject!: (e: Error) => void;
  const promise = new Promise<void>((r, j) => {
    resolve = r;
    reject = j;
  });
  return { promise, resolve, reject };
}
beforeEach(() => {
  (globalThis as any).IS_REACT_ACT_ENVIRONMENT = true;
  mocks.epoch = 0;
  vi.clearAllMocks();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});
it("late A history load cannot prune A with the departed viewport's protected turns", async () => {
  const a = deferred(),
    b = deferred();
  mocks.load.mockReturnValueOnce(a.promise).mockReturnValueOnce(b.promise);
  await render("A");
  await render("B");
  await act(async () => a.resolve());
  expect(mocks.prune).not.toHaveBeenCalled();
  await act(async () => b.resolve());
  expect(mocks.prune).toHaveBeenCalledOnce();
  expect(mocks.prune.mock.calls[0][0]).toBe("B");
});
it("A→B→A rejects the first A history generation even with the same turn ordinal", async () => {
  const old = deferred(),
    b = deferred(),
    current = deferred();
  mocks.load
    .mockReturnValueOnce(old.promise)
    .mockReturnValueOnce(b.promise)
    .mockReturnValueOnce(current.promise);
  await render("A");
  await render("B");
  await render("A");
  await act(async () => old.resolve());
  expect(mocks.prune).not.toHaveBeenCalled();
  await act(async () => current.resolve());
  expect(mocks.prune).toHaveBeenCalledOnce();
});
it("same-session replace revision rejects an older history load", async () => {
  const old = deferred(),
    current = deferred();
  mocks.load
    .mockReturnValueOnce(old.promise)
    .mockReturnValueOnce(current.promise);
  await render("A");
  mocks.epoch = 1;
  await render("A");
  await act(async () => old.resolve());
  expect(mocks.prune).not.toHaveBeenCalled();
  await act(async () => current.resolve());
  expect(mocks.prune).toHaveBeenCalledOnce();
});
it("legal explicit A end/page selection remains scoped and survives a B visit", async () => {
  mocks.load.mockImplementation(() => new Promise(() => {}));
  await render("A");
  await act(async () =>
    value.setTurnPageSelection({ sessionId: "A", pageIndex: 2 })
  );
  expect(value.selectedTurnPageIndex).toBe(2);
  await render("B");
  expect(value.selectedTurnPageIndex).toBe(Number.MAX_SAFE_INTEGER);
  await render("A");
  expect(value.selectedTurnPageIndex).toBe(2);
});

it("each parallel session retains its own explicit history end/page selection", async () => {
  mocks.load.mockImplementation(() => new Promise(() => {}));
  await render("A");
  await act(async () =>
    value.setTurnPageSelection({ sessionId: "A", pageIndex: 2 })
  );
  await render("B");
  await act(async () =>
    value.setTurnPageSelection({ sessionId: "B", pageIndex: 1 })
  );
  await render("A");
  expect(value.selectedTurnPageIndex).toBe(2);
  await render("B");
  expect(value.selectedTurnPageIndex).toBe(1);
  await act(async () =>
    value.setTurnPageSelection({ sessionId: "B", pageIndex: null })
  );
  await render("A");
  expect(value.selectedTurnPageIndex).toBe(2);
});
