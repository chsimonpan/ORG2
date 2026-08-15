import type { OptimizedChatItem } from "../chatItemPipeline/types";
import type { SearchResult } from "../hooks/useChatSearch";

export type ChatSearchRole = "user" | "assistant";

function appendEventId(ids: string[], id: string | null | undefined): void {
  if (id && !ids.includes(id)) ids.push(id);
}

/** Event ids represented by one rendered row, including consolidated groups. */
export function getChatSearchEventIds(item: OptimizedChatItem): string[] {
  const ids: string[] = [];
  appendEventId(ids, item.event?.id);
  item.readFileEvents?.forEach((event) => appendEventId(ids, event.id));
  item.activityStackGroup?.events.forEach((event) =>
    appendEventId(ids, event.id)
  );
  item.actionSummaryItems?.forEach(({ event }) => appendEventId(ids, event.id));
  return ids;
}

export function getChatSearchRole(item: OptimizedChatItem): ChatSearchRole {
  return item.event?.source === "user" ? "user" : "assistant";
}

/** Resolve a clicked rendered match back to the exact search-result position. */
export function findChatSearchResultIndex(
  results: readonly SearchResult[],
  representedEventIds: readonly string[]
): number {
  const represented = new Set(representedEventIds);
  return results.findIndex((result) => represented.has(result.item.id));
}
