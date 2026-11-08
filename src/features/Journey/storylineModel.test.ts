/* eslint-disable @typescript-eslint/no-explicit-any -- fixtures are partial snapshots */
import { describe, expect, it } from "vitest";
import { describe as _d, expect as _e, it as _i } from "vitest";

import { shortTitle, toStoryline } from "./storylineModel";
import { toStoryline as _ts } from "./storylineModel";
import { UNTITLED as _U } from "./storylineModel";
import { COMPACTED as _C } from "./storylineModel";

const anchor = (id: string) => ({
  ownerSessionId: "o",
  sourceSessionId: "o",
  branchId: "b",
  durableMessageId: id,
  boundaryVersion: 1,
  boundaryHash: "h" + id,
});
const snap = (n: number) => ({
  schemaVersion: 1 as const,
  ownerSessionId: "o",
  revision: 1,
  edges: [],
  capabilities: { read: true, fork: false, import: false, mutate: false },
  nodes: Array.from({ length: n }, (_, i) => ({
    id: "n" + i,
    title: "问题" + i + " 补充说明：很长很长",
    timestamp: 1000 + i * 60000,
    laneId: "l",
    kind: "question" as const,
    anchor: anchor("m" + i),
  })),
});
describe("storyline adapter", () => {
  it("truncates raw message text and strips the repeated padding", () => {
    expect(shortTitle("abc 补充说明：xxxx")).toBe("abc");
    expect(shortTitle("一二三四五六七八九十一二三四五六七八九十").length).toBe(
      17
    );
  });
  it("every durable node becomes exactly one main-thread event, no loss", () => {
    const m = toStoryline(snap(7), null);
    expect(m.events.length).toBe(7);
    expect(m.coverage).toEqual({ nodes: 7, shown: 7, hidden: 0 });
    expect(m.threads.length).toBe(1);
  });
  it("fork branch becomes a child thread anchored on an existing durable message", () => {
    const m = toStoryline(snap(3), {
      branches: {
        main: { id: "main", name: "主干", state: "active" },
        f: {
          id: "f",
          name: "fork",
          parent_branch_id: "main",
          parent_anchor_message_id: "m1",
          state: "closed",
        },
      },
      reviews: { r: { id: "r", fork_id: "f", state: "confirmed" } },
    });
    expect(m.threads.map((t) => t.id)).toEqual(["main", "f"]);
    expect(m.threads[1].parent).toBe("main");
    expect(m.threads[1].fade).toBe(true);
    expect(
      m.events.some((e) => e.id === "review:r" && e.type === "verify")
    ).toBe(true);
  });
  it("fork whose parent anchor is not durable is skipped, never invented", () => {
    const m = toStoryline(snap(2), {
      branches: {
        f: {
          id: "f",
          name: "x",
          parent_branch_id: "main",
          parent_anchor_message_id: "missing",
          state: "closed",
        },
      },
    });
    expect(m.threads.length).toBe(1);
  });
});

_d("titles override", () => {
  _i("uses digest title when present", () => {
    const snap: any = {
      ownerSessionId: "s",
      revision: 1,
      nodes: [
        {
          id: "n1",
          kind: "question",
          title: "原文很长很长很长很长很长很长很长很长",
          timestamp: 1000,
          laneId: "l",
          anchor: {
            ownerSessionId: "s",
            durableMessageId: "m1",
            boundaryHash: "h",
          },
        },
      ],
      edges: [],
      capabilities: {},
    };
    const m = _ts(snap, null, { n1: "导出账单" });
    _e(m.events.find((e) => e.id === "n1")?.title).toBe("导出账单");
  });
});

_d("no fallback titles", () => {
  _i(
    "a node without a model title is marked untitled, never auto-truncated text",
    () => {
      const snap: any = {
        ownerSessionId: "s",
        schemaVersion: 1,
        revision: 1,
        nodes: [
          {
            id: "n1",
            kind: "question",
            title: "原文很长很长很长很长很长很长很长很长",
            timestamp: 1000,
            laneId: "l",
            anchor: {
              ownerSessionId: "s",
              durableMessageId: "m1",
              boundaryHash: "h",
            },
          },
        ],
        edges: [],
        capabilities: { read: true, fork: false, import: false, mutate: false },
      };
      const m = _ts(snap, null, {});
      const e = m.events.find((x) => x.id === "n1");
      _e(e?.title).toBe(_U);
      _e(e?.summary).toBe("原文很长很长很长很长很长很长很长很长");
    }
  );
});

_d("compaction rows", () => {
  _i(
    "a compaction node gets a fixed label, not pending, and ignores any stored title",
    () => {
      const snap: any = {
        ownerSessionId: "s",
        schemaVersion: 1,
        revision: 1,
        nodes: [
          {
            id: "c1",
            kind: "verify",
            title: "Context compacted",
            timestamp: 1000,
            laneId: "l",
            anchor: {
              ownerSessionId: "s",
              sourceSessionId: "s",
              branchId: "b",
              durableMessageId: "c1",
              boundaryVersion: 1,
              boundaryHash: "h",
            },
          },
        ],
      };
      const e = _ts(snap, null, {}).events[0];
      _e(e.title).toBe(_C);
      _e(e.summary).toBe("Context compacted");
    }
  );
});
