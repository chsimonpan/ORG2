import { describe, expect, it } from "vitest";

import type { SessionEvent } from "@src/engines/SessionCore/core/types";

import { buildChatSearchEventIndex } from "./useChatSearch";
import { closeChatSearch } from "./useChatSearchIntegration";

describe("buildChatSearchEventIndex", () => {
  it("maps both the canonical event id and Rust's preferred chunk id", () => {
    const events = [
      { id: "event-1", chunk_id: "chunk-1" },
      { id: "event-2", chunk_id: null },
    ] as SessionEvent[];

    const index = buildChatSearchEventIndex(events);

    expect(index.get("event-1")).toBe(0);
    expect(index.get("chunk-1")).toBe(0);
    expect(index.get("event-2")).toBe(1);
  });

  it("clears search state before hiding so close then reopen starts clean", () => {
    const calls: string[] = [];
    closeChatSearch(
      () => calls.push("clear"),
      (visible) => calls.push(`visible:${visible}`)
    );
    expect(calls).toEqual(["clear", "visible:false"]);
  });
});
