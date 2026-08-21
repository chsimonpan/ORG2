// @vitest-environment jsdom
import { describe, expect, it } from "vitest";

import type { SessionEvent } from "@src/engines/SessionCore/core/types";

import { resolveLatestJourneyUserMessageId } from "./ChatView";

const userEvent = (sessionId: string, id: string): SessionEvent =>
  ({
    id,
    sessionId,
    source: "user",
    functionName: "user_message",
    result: {},
  }) as SessionEvent;

describe("resolveLatestJourneyUserMessageId", () => {
  it("never borrows the previous session anchor during a session switch", () => {
    expect(
      resolveLatestJourneyUserMessageId(
        [userEvent("previous-session", "previous-user")],
        "aug-session"
      )
    ).toBeNull();
  });

  it("selects the latest backend user message belonging to the requested session", () => {
    expect(
      resolveLatestJourneyUserMessageId(
        [
          userEvent("aug-session", "aug-user-1"),
          userEvent("previous-session", "previous-user"),
          userEvent("aug-session", "aug-user-2"),
        ],
        "aug-session"
      )
    ).toBe("aug-user-2");
  });
});
