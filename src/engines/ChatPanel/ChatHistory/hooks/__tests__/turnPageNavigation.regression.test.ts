import { describe, expect, it } from "vitest";

import type { OptimizedChatItem } from "../../chatItemPipeline/types";
import {
  type ChatTurnPaginationOptions,
  projectChatTurnPagination,
} from "../useChatTurnPagination";
import {
  selectNextTurnPage,
  selectPreviousTurnPage,
} from "../useTurnPageSelection";

function item(id: string): OptimizedChatItem {
  return { chunk_id: id, type: "activity" } as OptimizedChatItem;
}

function historyPage(activePageIndex: number) {
  const flatItems = [item("old"), item("middle"), item("latest")];
  const options: ChatTurnPaginationOptions = {
    enabled: true,
    activePageIndex,
    groupCounts: [1, 1, 1],
    groupHeaders: [
      item("header-old"),
      item("header-middle"),
      item("header-latest"),
    ],
    groupMeta: [{}, {}, {}] as ChatTurnPaginationOptions["groupMeta"],
    flatItems,
    lastAssistantFlatIndexPerItem: [null, null, null],
  };
  return projectChatTurnPagination(options);
}

describe("turn-page arrow navigation", () => {
  it("moves historical cursor and rendered page in both directions", () => {
    const sessionId = "agent-session-history";
    const latest = historyPage(Number.MAX_SAFE_INTEGER);
    expect(latest.currentPageIndex).toBe(2);
    expect(latest.displayFlatItems.map((event) => event.chunk_id)).toEqual([
      "latest",
    ]);

    const previousCursor = selectPreviousTurnPage({
      activeId: sessionId,
      currentPageIndex: latest.currentPageIndex,
    });
    expect(previousCursor).toEqual({ pageIndex: 1, sessionId });
    const middle = historyPage(
      previousCursor.pageIndex ?? Number.MAX_SAFE_INTEGER
    );
    expect(middle.currentPageIndex).toBe(1);
    expect(middle.displayFlatItems.map((event) => event.chunk_id)).toEqual([
      "middle",
    ]);

    const firstCursor = selectPreviousTurnPage({
      activeId: sessionId,
      currentPageIndex: middle.currentPageIndex,
    });
    expect(firstCursor).toEqual({ pageIndex: 0, sessionId });
    const first = historyPage(firstCursor.pageIndex ?? Number.MAX_SAFE_INTEGER);
    expect(first.displayFlatItems.map((event) => event.chunk_id)).toEqual([
      "old",
    ]);

    const nextCursor = selectNextTurnPage({
      activeId: sessionId,
      currentPageIndex: first.currentPageIndex,
      pageCount: first.pageCount,
    });
    expect(nextCursor).toEqual({ pageIndex: 1, sessionId });
    const middleAgain = historyPage(
      nextCursor.pageIndex ?? Number.MAX_SAFE_INTEGER
    );
    expect(middleAgain.displayFlatItems.map((event) => event.chunk_id)).toEqual(
      ["middle"]
    );

    const latestCursor = selectNextTurnPage({
      activeId: sessionId,
      currentPageIndex: middleAgain.currentPageIndex,
      pageCount: middleAgain.pageCount,
    });
    expect(latestCursor).toEqual({ pageIndex: null, sessionId });
    const latestAgain = historyPage(
      latestCursor.pageIndex ?? Number.MAX_SAFE_INTEGER
    );
    expect(latestAgain.currentPageIndex).toBe(2);
    expect(latestAgain.displayFlatItems.map((event) => event.chunk_id)).toEqual(
      ["latest"]
    );
  });
});
