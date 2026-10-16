import { describe, expect, it } from "vitest";

import type { SessionEvent } from "@src/engines/SessionCore/core/types";

import { timelineBeforeTurn } from "./localConversationTurnIdentity";

function event(
  id: string,
  source: SessionEvent["source"],
  turnIntentId?: string
): SessionEvent {
  return {
    id,
    chunk_id: id,
    sessionId: "root",
    createdAt: "2026-10-16T00:00:00Z",
    functionName: source === "user" ? "user_message" : "assistant",
    actionType: source === "user" ? "raw" : "assistant",
    uiCanonical: "",
    args: {},
    result: turnIntentId ? { turnIntentId } : {},
    source,
    displayText: "same",
    displayStatus: "completed",
    displayVariant: "message",
    activityStatus: "agent",
  } as SessionEvent;
}

describe("timelineBeforeTurn", () => {
  it("retry recovery drops the settled turn and its replies", () => {
    const timeline = [
      event("u1", "user", "intent-1"),
      event("a1", "assistant"),
      event("u2", "user", "intent-2"),
      event("a2", "assistant"),
    ];
    expect(timelineBeforeTurn(timeline, "intent-2").map((e) => e.id)).toEqual([
      "u1",
      "a1",
    ]);
  });

  it("keeps an earlier identical-text turn with a different id", () => {
    const timeline = [
      event("u1", "user", "intent-1"),
      event("a1", "assistant"),
      event("pending", "user", "intent-2"),
    ];
    expect(timelineBeforeTurn(timeline, "intent-2").map((e) => e.id)).toEqual([
      "u1",
      "a1",
    ]);
  });

  it("legacy rows without ids are never treated as this turn", () => {
    const timeline = [event("legacy", "user"), event("a0", "assistant")];
    expect(timelineBeforeTurn(timeline, "intent-9")).toHaveLength(2);
  });
});
