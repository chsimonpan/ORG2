import { describe, expect, it } from "vitest";

import type { SessionEvent } from "@src/engines/SessionCore/core/types";

import type { OptimizedChatItem } from "../chatItemPipeline/types";
import type { SearchResult } from "../hooks/useChatSearch";
import {
  findChatSearchResultIndex,
  getChatSearchEventIds,
  getChatSearchRole,
} from "./chatSearchPresentation";

function event(id: string, source: SessionEvent["source"]): SessionEvent {
  return { id, source } as SessionEvent;
}

describe("chat search result presentation", () => {
  it("keeps user and assistant result rows visually distinguishable", () => {
    expect(
      getChatSearchRole({ event: event("u", "user") } as OptimizedChatItem)
    ).toBe("user");
    expect(
      getChatSearchRole({ event: event("a", "assistant") } as OptimizedChatItem)
    ).toBe("assistant");
  });

  it("maps consolidated rows to every represented event for click navigation", () => {
    const item = {
      event: event("primary", "assistant"),
      readFileEvents: [event("read-1", "assistant")],
      activityStackGroup: {
        category: "browser",
        events: [event("stack-1", "assistant")],
      },
      actionSummaryItems: [
        { category: "explore", event: event("summary-1", "assistant") },
      ],
    } as unknown as OptimizedChatItem;

    expect(getChatSearchEventIds(item)).toEqual([
      "primary",
      "read-1",
      "stack-1",
      "summary-1",
    ]);
  });

  it("resolves a clicked row to the exact result index", () => {
    const results = ["first", "read-1", "last"].map(
      (id, index) => ({ item: event(id, "assistant"), index }) as SearchResult
    );

    expect(findChatSearchResultIndex(results, ["primary", "read-1"])).toBe(1);
    expect(findChatSearchResultIndex(results, ["missing"])).toBe(-1);
  });
});
