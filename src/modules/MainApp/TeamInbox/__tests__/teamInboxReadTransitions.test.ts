import { describe, expect, it, vi } from "vitest";

import type { TeamInboxItem } from "../domain";
import { performTeamInboxReadTransition } from "../teamInboxReadTransitions";

function assignedItem(readAt: string | null): TeamInboxItem {
  return {
    kind: "assigned_work_item",
    id: "wi-1",
    occurredAt: "2026-07-29T00:00:00.000Z",
    readAt,
    workItem: {
      id: "wi-1",
      title: "Ship the thing",
      status: "todo",
      priority: "medium",
    },
  } as unknown as TeamInboxItem;
}

describe("performTeamInboxReadTransition", () => {
  it("resolves ok without touching refresh on the success path", async () => {
    const markRead = vi.fn().mockResolvedValue(undefined);
    const refresh = vi.fn().mockResolvedValue(undefined);

    const result = await performTeamInboxReadTransition(
      "read",
      assignedItem(null),
      { markRead, refresh }
    );

    expect(result).toEqual({ ok: true, resyncStarted: false });
    expect(markRead).toHaveBeenCalledTimes(1);
    expect(refresh).not.toHaveBeenCalled();
  });

  it("triggers a data source refresh when the mutation rejects", async () => {
    const markRead = vi.fn().mockRejectedValue(new Error("network down"));
    const refresh = vi.fn().mockResolvedValue(undefined);

    const result = await performTeamInboxReadTransition(
      "read",
      assignedItem(null),
      { markRead, refresh }
    );

    expect(result).toEqual({ ok: false, resyncStarted: true });
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it("still reports resyncStarted even if the refresh itself fails", async () => {
    const markUnread = vi
      .fn()
      .mockRejectedValue(new Error("no longer visible"));
    const refresh = vi.fn().mockRejectedValue(new Error("offline"));

    const result = await performTeamInboxReadTransition(
      "unread",
      assignedItem("2026-07-28T00:00:00.000Z"),
      { markUnread, refresh }
    );

    expect(result).toEqual({ ok: false, resyncStarted: true });
    expect(markUnread).toHaveBeenCalledTimes(1);
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it("does not attempt a resync when the data source has no refresh", async () => {
    const markRead = vi.fn().mockRejectedValue(new Error("boom"));

    const result = await performTeamInboxReadTransition(
      "read",
      assignedItem(null),
      { markRead }
    );

    expect(result).toEqual({ ok: false, resyncStarted: false });
  });

  it("is a no-op when the data source has no mutator for the requested kind", async () => {
    const refresh = vi.fn();

    const result = await performTeamInboxReadTransition(
      "unread",
      assignedItem(null),
      { refresh }
    );

    expect(result).toEqual({ ok: true, resyncStarted: false });
    expect(refresh).not.toHaveBeenCalled();
  });
});
