// @vitest-environment jsdom
import { Provider, createStore, useAtomValue } from "jotai";
import React, { act } from "react";
import { type Root, createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { derivedSnapshotAtom, eventStoreVersionAtom } from "../../atoms/events";
import { sessionIdAtom } from "../../atoms/metadata";
import { useEventStoreBridge } from "../useEventStoreBridge";

const mock = vi.hoisted(() => ({
  listener: null as null | ((snapshot: any, sessionId: string) => void),
  cache: new Map<string, any>(),
  getSnapshot: vi.fn(),
}));
vi.mock("../../atoms/events", async () => {
  const { atom } = await import("jotai");
  return {
    derivedSnapshotAtom: atom<any>(null),
    eventStoreVersionAtom: atom(0),
  };
});
vi.mock("../../atoms/metadata", async () => {
  const { atom } = await import("jotai");
  return { sessionIdAtom: atom<string | null>(null) };
});
vi.mock("@src/hooks/logger", () => ({
  createLogger: () => ({ warn: vi.fn() }),
}));
vi.mock("../EventStoreProxy", () => ({
  isStreamingSnapshot: (s: any) => s.streaming === true,
  eventStoreProxy: {
    init: () => Promise.resolve(),
    detachTauri: vi.fn(),
    getLatestSessionSnapshot: (id: string) => mock.cache.get(id),
    getSnapshot: mock.getSnapshot,
    subscribe: (listener: typeof mock.listener) => {
      mock.listener = listener;
      return () => {
        mock.listener = null;
      };
    },
  },
}));
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}
function full(id: string, version = 1) {
  const event = { id: `${id}-message`, sessionId: id, displayText: id };
  return {
    version,
    events: [event],
    eventIndex: { [event.id]: 0 },
    chatEvents: [event],
    sortedSimulatorEvents: [event],
    eventCount: 1,
  };
}
function stream(id: string, version = 2) {
  const base = full(id, version);
  return {
    ...base,
    streaming: true,
    events: undefined,
    eventIndex: undefined,
    simulatorEventUpserts: base.chatEvents,
    sortedSimulatorEventIds: [base.chatEvents[0].id],
  };
}
let root: Root;
let host: HTMLDivElement;
let store: ReturnType<typeof createStore>;
function Harness() {
  useEventStoreBridge();
  const snapshot = useAtomValue(derivedSnapshotAtom);
  const sessionId = useAtomValue(sessionIdAtom);
  return React.createElement(
    "section",
    {},
    ...["A", "B"].map((id) =>
      React.createElement(
        "button",
        {
          key: id,
          "data-session": id,
          onClick: () => store.set(sessionIdAtom, id),
        },
        id
      )
    ),
    React.createElement(
      "output",
      { "data-owner": sessionId ?? "none" },
      ...(snapshot?.chatEvents ?? []).map((event: any) =>
        React.createElement(
          "span",
          {
            key: event.id,
            "data-transcript-anchor-id": event.id,
          },
          event.displayText
        )
      )
    )
  );
}
async function mount(id: string | null = "A") {
  store.set(sessionIdAtom, id);
  await act(async () =>
    root.render(
      React.createElement(Provider, { store }, React.createElement(Harness))
    )
  );
}
async function switchTo(id: string) {
  await act(async () =>
    host.querySelector<HTMLButtonElement>(`[data-session="${id}"]`)!.click()
  );
}
async function emit(snapshot: any, id: string) {
  await act(async () => mock.listener!(snapshot, id));
}
beforeEach(() => {
  (globalThis as any).IS_REACT_ACT_ENVIRONMENT = true;
  mock.cache.clear();
  mock.getSnapshot.mockReset();
  mock.listener = null;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  store = createStore();
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});
describe("real bridge ReactDOM session switch isolation", () => {
  it("clears uncached B immediately without presenting A message anchors", async () => {
    mock.cache.set("A", full("A"));
    await mount();
    await switchTo("B");
    expect(store.get(derivedSnapshotAtom)).toBeNull();
    expect(
      host.querySelector('[data-transcript-anchor-id="A-message"]')
    ).toBeNull();
    expect(store.get(eventStoreVersionAtom)).toBe(0);
  });
  it("rejects inactive-session pushes even before React commits the new atom value", async () => {
    mock.cache.set("A", full("A"));
    mock.cache.set("B", full("B"));
    await mount();
    const owners: string[] = [];
    const unsubscribe = store.sub(derivedSnapshotAtom, () => {
      const snap = store.get(derivedSnapshotAtom);
      if (snap) owners.push(snap.chatEvents[0].sessionId);
    });
    await act(async () => {
      store.set(sessionIdAtom, "B");
      mock.listener!(full("A", 99), "A");
    });
    unsubscribe();
    expect(owners).not.toContain("A");
    expect(store.get(derivedSnapshotAtom)?.chatEvents[0].sessionId).toBe("B");
    expect(store.get(eventStoreVersionAtom)).toBe(1);
  });
  it("does not accept any parallel stream while no session is selected", async () => {
    await mount(null);
    await emit(full("B"), "B");
    expect(store.get(derivedSnapshotAtom)).toBeNull();
  });
  it("A→B→A rejects old A hydration, retries new scope, and preserves current stream", async () => {
    const old = deferred<any>(),
      current = deferred<any>();
    mock.getSnapshot
      .mockReturnValueOnce(old.promise)
      .mockReturnValueOnce(current.promise);
    await mount();
    await emit(stream("A", 2), "A");
    await switchTo("B");
    await emit(full("B"), "B");
    await switchTo("A");
    await emit(stream("A", 8), "A");
    expect(mock.getSnapshot).toHaveBeenCalledTimes(2);
    await act(async () => old.resolve(full("A", 1)));
    expect(store.get(eventStoreVersionAtom)).toBe(8);
    await act(async () => current.resolve(full("A", 7)));
    expect(store.get(eventStoreVersionAtom)).toBe(8);
    expect(store.get(derivedSnapshotAtom)?.chatEvents[0].id).toBe("A-message");
    expect(
      host.querySelector('[data-transcript-anchor-id="B-message"]')
    ).toBeNull();
  });
  it("a failed old A request cannot clear a newer A request token", async () => {
    const old = deferred<any>(),
      current = deferred<any>();
    mock.getSnapshot
      .mockReturnValueOnce(old.promise)
      .mockReturnValueOnce(current.promise);
    await mount();
    await emit(stream("A", 2), "A");
    // No intermediate React commit: request scopes must still observe both changes.
    await act(async () => {
      store.set(sessionIdAtom, "B");
      store.set(sessionIdAtom, "A");
    });
    await emit(stream("A", 9), "A");
    await act(async () => old.reject(new Error("stale")));
    await emit(stream("A", 10), "A");
    expect(mock.getSnapshot).toHaveBeenCalledTimes(2);
    await act(async () => current.resolve(full("A", 8)));
    expect(store.get(eventStoreVersionAtom)).toBe(10);
  });
  it("hydrates older full history without losing the latest live event", async () => {
    const request = deferred<any>();
    mock.getSnapshot.mockReturnValue(request.promise);
    await mount();
    await emit(stream("A", 12), "A");
    const baseline = full("A", 5);
    baseline.events[0].id = "A-earlier-message";
    baseline.eventIndex = { "A-earlier-message": 0 };
    await act(async () => request.resolve(baseline));
    const snapshot = store.get(derivedSnapshotAtom);
    expect(snapshot?.events.map((event: any) => event.id)).toEqual([
      "A-earlier-message",
      "A-message",
    ]);
    expect(snapshot?.chatEvents[0].id).toBe("A-message");
    expect(store.get(eventStoreVersionAtom)).toBe(12);
  });
  it("a newer pushed baseline wins over a late older hydration", async () => {
    const request = deferred<any>();
    mock.getSnapshot.mockReturnValue(request.promise);
    await mount();
    await emit(stream("A", 3), "A");
    await emit(full("A", 15), "A");
    await act(async () => request.resolve(full("A", 1)));
    expect(store.get(eventStoreVersionAtom)).toBe(15);
  });
  it("does not write a late hydration after unmount", async () => {
    const request = deferred<any>();
    mock.getSnapshot.mockReturnValue(request.promise);
    await mount();
    await emit(stream("A", 4), "A");
    await act(async () => root.unmount());
    await act(async () => request.resolve(full("A", 1)));
    expect(store.get(eventStoreVersionAtom)).toBe(4);
  });
  it("keeps per-session cached message anchors through legal A→B→A navigation", async () => {
    mock.cache.set("A", full("A", 10));
    mock.cache.set("B", full("B", 3));
    await mount();
    await switchTo("B");
    await emit(full("A", 11), "A");
    expect(
      host.querySelector('[data-transcript-anchor-id="A-message"]')
    ).toBeNull();
    await switchTo("A");
    expect(
      host.querySelector('[data-transcript-anchor-id="A-message"]')
    ).not.toBeNull();
    expect(store.get(eventStoreVersionAtom)).toBe(10);
  });
});
