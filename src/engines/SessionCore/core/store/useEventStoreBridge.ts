/**
 * Hook to bridge Rust EventStore → Jotai atoms.
 *
 * Mount this ONCE at the app root (or session workspace root).
 * It initializes the EventStoreProxy's Tauri event listener and
 * subscribes to snapshot pushes from Rust, updating the Jotai atoms.
 *
 * Only snapshots whose `sessionId` matches the current active session
 * are forwarded to Jotai atoms — child session snapshots (e.g. subagent
 * nested events) are routed to per-session subscribers instead.
 *
 * Snapshot merging:
 * Rust pushes a lightweight StreamingSnapshot during active streaming (no
 * `events` / `sortedSimulatorEvents` to reduce serialization cost). When we
 * receive one we merge it on top of the previous DerivedSnapshot so that the
 * Simulator always has the full event list while chat gets the latest updates.
 */
import { useSetAtom, useStore } from "jotai";
import { useEffect, useLayoutEffect, useRef } from "react";

import { createLogger } from "@src/hooks/logger";

import { derivedSnapshotAtom, eventStoreVersionAtom } from "../atoms/events";
import { sessionIdAtom } from "../atoms/metadata";
import type { SessionEvent } from "../types";
import {
  type DerivedSnapshot,
  type Snapshot,
  type StreamingSnapshot,
  eventStoreProxy,
  isStreamingSnapshot,
} from "./EventStoreProxy";

const log = createLogger("useEventStoreBridge");

function applyEventUpserts(
  previousEvents: SessionEvent[],
  previousEventIndex: Record<string, number>,
  upserts: SessionEvent[]
): { events: SessionEvent[]; appended: boolean } {
  if (upserts.length === 0) return { events: previousEvents, appended: false };
  let nextEvents: SessionEvent[] | null = null;
  let appended = false;

  for (const upsert of upserts) {
    const existingIndex = previousEventIndex[upsert.id];
    if (existingIndex !== undefined) {
      if (previousEvents[existingIndex] !== upsert) {
        nextEvents ??= [...previousEvents];
        nextEvents[existingIndex] = upsert;
      }
    } else {
      nextEvents ??= [...previousEvents];
      nextEvents.push(upsert);
      appended = true;
    }
  }

  return { events: nextEvents ?? previousEvents, appended };
}

function buildEventIndex(events: SessionEvent[]): Record<string, number> {
  const eventIndex: Record<string, number> = {};
  for (let index = 0; index < events.length; index++) {
    eventIndex[events[index].id] = index;
  }
  return eventIndex;
}

function eventsForIds(
  events: SessionEvent[],
  eventIndex: Record<string, number>,
  ids: string[]
): SessionEvent[] {
  return ids
    .map((id) => {
      const index = eventIndex[id];
      return index === undefined ? undefined : events[index];
    })
    .filter((event): event is SessionEvent => Boolean(event));
}

function mergeStreamingBaseline(
  prev: DerivedSnapshot,
  snapshot: StreamingSnapshot
): DerivedSnapshot & { streaming: true } {
  const { events, appended } = applyEventUpserts(
    prev.events,
    prev.eventIndex,
    snapshot.simulatorEventUpserts ?? []
  );
  // In-place replacements keep every id → index mapping valid;
  // only appends (membership growth) require rebuilding.
  const eventIndex = appended ? buildEventIndex(events) : prev.eventIndex;
  const sortedSimulatorEvents =
    snapshot.sortedSimulatorEventIds &&
    snapshot.sortedSimulatorEventIds.length > 0
      ? eventsForIds(events, eventIndex, snapshot.sortedSimulatorEventIds)
      : prev.sortedSimulatorEvents;
  const merged: DerivedSnapshot & { streaming: true } = {
    ...prev,
    version: snapshot.version,
    eventCount: snapshot.eventCount,
    events,
    eventIndex,
    chatEvents: snapshot.chatEvents,
    sortedSimulatorEvents,
    lastEvent: snapshot.lastEvent,
    hasRunningEvent: snapshot.hasRunningEvent,
    sortedSimulatorEventIds: snapshot.sortedSimulatorEventIds,
    eventPreviewById: snapshot.eventPreviewById,
    createdAtById: snapshot.createdAtById,
    threadIdById: snapshot.threadIdById,
    functionNameById: snapshot.functionNameById,
    displayStatusById: snapshot.displayStatusById,
    displayVariantById: snapshot.displayVariantById,
    streaming: true,
  };
  return merged;
}

export function useEventStoreBridge(): void {
  const setSnapshot = useSetAtom(derivedSnapshotAtom);
  const setVersion = useSetAtom(eventStoreVersionAtom);
  const store = useStore();
  const activeSessionIdRef = useRef<string | null>(null);
  const scopeRevisionRef = useRef(0);
  const lastDerivedRef = useRef<DerivedSnapshot | null>(null);
  const latestStreamingRef = useRef<StreamingSnapshot | null>(null);
  // Token identity, not just session ID: A→B→A must not reuse A's old request.
  const hydrationRequestedRef = useRef(new Map<string, object>());

  useLayoutEffect(() => {
    const resetScope = () => {
      const sessionId = store.get(sessionIdAtom);
      activeSessionIdRef.current = sessionId;
      scopeRevisionRef.current += 1;
      lastDerivedRef.current = null;
      latestStreamingRef.current = null;
      hydrationRequestedRef.current.clear();
      const cached = sessionId
        ? eventStoreProxy.getLatestSessionSnapshot(sessionId)
        : null;
      if (cached) {
        if (isStreamingSnapshot(cached)) latestStreamingRef.current = cached;
        else lastDerivedRef.current = cached as DerivedSnapshot;
      }
      // A new uncached session must never expose the departing session's atoms.
      setSnapshot(cached ?? null);
      setVersion(cached?.version ?? 0);
    };
    resetScope();
    // Jotai changes synchronously, before React's passive effect/commit. This
    // also records A→B→A changes batched into a single render.
    return store.sub(sessionIdAtom, resetScope);
  }, [store, setSnapshot, setVersion]);

  useEffect(() => {
    let destroyed = false;
    let unsubscribe: (() => void) | null = null;

    eventStoreProxy.init().then(() => {
      // Guard against fast unmount: if the cleanup already ran before the
      // promise resolved, skip adding the listener so it is never orphaned
      // in _globalListeners holding stale setter closures.
      if (destroyed) return;

      unsubscribe = eventStoreProxy.subscribe(
        (snapshot: Snapshot, sessionId: string) => {
          if (
            destroyed ||
            !store.get(sessionIdAtom) ||
            sessionId !== store.get(sessionIdAtom) ||
            sessionId !== activeSessionIdRef.current
          ) {
            return;
          }

          if (isStreamingSnapshot(snapshot)) {
            latestStreamingRef.current = snapshot;
            // StreamingSnapshot omits the full `events` array and `eventIndex`
            // for perf. Merge bounded simulator upserts onto the last
            // DerivedSnapshot so active simulator windows can hydrate live events.
            //
            // IMPORTANT: the merged object keeps `streaming: true` so that
            // chatEventsAtom's stability optimisation (allArgsStable) is
            // bypassed. Without this flag, changes to args.streamContent
            // (tool-call argument streaming — e.g. create_plan content) are
            // invisible to the stability check and the plan card never updates
            // mid-stream, showing "(plan is empty)" until the turn completes.
            const prev = lastDerivedRef.current;
            if (prev) {
              const merged = mergeStreamingBaseline(prev, snapshot);
              lastDerivedRef.current = merged;
              setSnapshot(merged);
              setVersion(merged.version);
              return;
            }
            // No prior DerivedSnapshot yet — the StreamingSnapshot only
            // carries a bounded window of events (chatEvents + capped
            // simulator upserts), so rendering it alone would truncate the
            // session history. Pull the full derived snapshot once per
            // session as a baseline; subsequent streaming snapshots merge on
            // top of it via the branch above.
            if (!hydrationRequestedRef.current.has(sessionId)) {
              const token = {};
              const revision = scopeRevisionRef.current;
              hydrationRequestedRef.current.set(sessionId, token);
              const isCurrentRequest = () =>
                !destroyed &&
                revision === scopeRevisionRef.current &&
                store.get(sessionIdAtom) === sessionId &&
                hydrationRequestedRef.current.get(sessionId) === token;
              void eventStoreProxy
                .getSnapshot(sessionId)
                .then((full) => {
                  if (!full || !isCurrentRequest()) {
                    return;
                  }
                  // Only apply if we still lack a derived baseline (a real
                  // DerivedSnapshot push may have landed in the meantime).
                  if (!lastDerivedRef.current) {
                    const live = latestStreamingRef.current;
                    const baseline =
                      live && live.version > full.version
                        ? mergeStreamingBaseline(full, live)
                        : full;
                    lastDerivedRef.current = baseline;
                    setSnapshot(baseline);
                    setVersion(baseline.version);
                  }
                })
                .catch((err) => {
                  // Allow a retry on the next streaming snapshot.
                  if (!isCurrentRequest()) return;
                  hydrationRequestedRef.current.delete(sessionId);
                  log.warn(
                    "[useEventStoreBridge] full snapshot hydration failed",
                    sessionId,
                    err
                  );
                })
                .finally(() => {
                  if (isCurrentRequest())
                    hydrationRequestedRef.current.delete(sessionId);
                });
            }
            // Fall through and store the streaming snapshot as-is so chat
            // stays live while hydration is in flight.
          } else {
            latestStreamingRef.current = null;
            lastDerivedRef.current = snapshot as DerivedSnapshot;
          }

          setSnapshot(snapshot);
          setVersion(snapshot.version);
        }
      );
    });

    return () => {
      destroyed = true;
      unsubscribe?.();
      // Tear down only the Tauri IPC listener so it is not orphaned when the
      // component unmounts before init() resolves (e.g. StrictMode double-mount,
      // fast navigation, HMR). detachTauri() keeps _sessionListeners and the
      // snapshot caches intact so other live consumers (subagent grids,
      // per-session subscribers) survive a bridge remount; full destroy() is
      // reserved for app exit / tests.
      eventStoreProxy.detachTauri();
    };
  }, [store, setSnapshot, setVersion]);
}
