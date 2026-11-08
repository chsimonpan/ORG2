// @vitest-environment jsdom
import { createElement } from "react";
import { describe, expect, it, vi } from "vitest";

import { createSmokeRoot, dispatch } from "@src/test/reactSmokeHarness";

import { JourneyStoryline } from "./JourneyStoryline";
import { JourneyStorylineFullscreen } from "./JourneyStorylineFullscreen";
import type { JourneySnapshot } from "./types";

const anchor = (id: string) => ({
  ownerSessionId: "o",
  sourceSessionId: "o",
  branchId: "b",
  durableMessageId: id,
  boundaryVersion: 1,
  boundaryHash: "h" + id,
});
// 36 nodes one minute apart: far more labels than one row can hold
const snap: JourneySnapshot = {
  schemaVersion: 1,
  ownerSessionId: "o",
  revision: 1,
  edges: [],
  capabilities: { read: true, fork: false, import: false, mutate: false },
  nodes: Array.from({ length: 36 }, (_, i) => ({
    id: "n" + i,
    title: "raw" + i,
    timestamp: 1_791_770_400_000 + i * 60000,
    laneId: "l",
    kind: "question" as const,
    anchor: anchor("m" + i),
  })),
};
const titles = Object.fromEntries(
  snap.nodes.map((n, i) => [
    n.id,
    i % 3 ? "账单导出系统细节设计第" + i + "项" : "幂等" + i,
  ])
);
const domain = {
  branches: {
    main: { id: "main", name: "主干", state: "active" },
    f: {
      id: "f",
      name: "journey-shot-fork",
      parent_branch_id: "main",
      parent_anchor_message_id: "m30",
      state: "closed",
    },
  },
  reviews: { r: { id: "r", fork_id: "f", state: "confirmed" } },
};
const boxes = (c: Element) =>
  [...c.querySelectorAll("[data-label-for]")].map((t) =>
    t.getAttribute("data-box")!.split(",").map(Number)
  );
const hit = (a: number[], b: number[]) =>
  a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3];
const tx = (g: Element | null) =>
  Number(/translate\(([-\d.]+),/.exec(g?.getAttribute("transform") ?? "")?.[1]);
describe("JourneyStoryline layout", () => {
  for (const variant of ["inline", "full"] as const)
    it(`${variant}: rendered milestone labels never overlap`, async () => {
      const root = createSmokeRoot();
      try {
        await root.render(
          createElement(JourneyStoryline, {
            snapshot: snap,
            domain,
            titles,
            onSelect: vi.fn(),
            variant,
          })
        );
        const bs = boxes(root.container);
        expect(bs.length).toBeGreaterThan(8);
        for (let a = 0; a < bs.length; a++)
          for (let b = a + 1; b < bs.length; b++)
            expect(hit(bs[a], bs[b])).toBe(false);
        expect(root.container.textContent).toMatch(/标签 \d+\/\d+/);
        const trunc = [
          ...root.container.querySelectorAll("[data-label-for]"),
        ].filter((t) => t.textContent!.endsWith("…"));
        if (variant === "inline") expect(trunc.length).toBeGreaterThan(0);
        for (const t of trunc)
          expect(t.parentElement!.querySelector("title")?.textContent).toMatch(
            /^(账单导出系统细节设计第\d+项|journey-shot-fork|复核 (confirmed|rejected|pending))$/
          );
      } finally {
        await root.unmount();
      }
    });
  it("fork sits at its anchor's real x; review step is labelled as untimed, never as a clock time", async () => {
    const root = createSmokeRoot();
    try {
      await root.render(
        createElement(JourneyStoryline, {
          snapshot: snap,
          domain,
          titles,
          onSelect: vi.fn(),
        })
      );
      const c = root.container;
      expect(tx(c.querySelector("[data-event-id='fork:f']"))).toBeCloseTo(
        tx(c.querySelector("[data-event-id=n30]")),
        3
      );
      expect(tx(c.querySelector("[data-event-id='review:r']"))).toBeGreaterThan(
        tx(c.querySelector("[data-event-id='fork:f']"))
      );
      expect(
        c.querySelector("[data-event-id='review:r'] > title")?.textContent
      ).toContain("无独立时间戳");
      expect(c.textContent).toContain("非时间刻度");
      expect(c.textContent).not.toMatch(/布局偏移/);
    } finally {
      await root.unmount();
    }
  });
  it("highlights exactly one current node (latest real node), selection shows its time", async () => {
    const sel = vi.fn(),
      root = createSmokeRoot();
    try {
      await root.render(
        createElement(JourneyStoryline, {
          snapshot: snap,
          domain,
          titles,
          onSelect: sel,
        })
      );
      const c = root.container;
      const cur = c.querySelectorAll("[data-current]");
      expect(cur.length).toBe(1);
      expect(cur[0].getAttribute("data-event-id")).toBe("n35");
      expect(
        c.querySelector("[data-label-for=n35]")?.getAttribute("font-weight")
      ).toBe("800");
      await dispatch(() =>
        (
          c.querySelector("[data-event-id=n3]") as unknown as HTMLElement
        ).dispatchEvent(new MouseEvent("click", { bubbles: true }))
      );
      expect(sel).toHaveBeenCalledWith(snap.nodes[3].anchor);
      expect(c.querySelector("[role=status]")?.textContent).toContain("幂等3");
      await dispatch(() =>
        (
          c.querySelector(
            "[aria-label='聚焦线程 journey-shot-fork']"
          ) as unknown as HTMLElement
        ).dispatchEvent(new MouseEvent("click", { bubbles: true }))
      );
      expect(
        c.querySelector("[data-event-id=n3]")?.getAttribute("opacity")
      ).toBe("0.12");
    } finally {
      await root.unmount();
    }
  });
  it("zooming widens the time span and re-runs label avoidance", async () => {
    const root = createSmokeRoot();
    try {
      await root.render(
        createElement(JourneyStoryline, {
          snapshot: snap,
          domain,
          titles,
          onSelect: vi.fn(),
        })
      );
      const c = root.container;
      const w0 = parseFloat(
          (c.querySelector("svg[role=img]") as SVGElement).style.width
        ),
        n0 = boxes(c).length;
      for (let i = 0; i < 3; i++)
        await dispatch(() =>
          (
            c.querySelector(
              "button[aria-label='横向放大']"
            ) as HTMLButtonElement
          ).click()
        );
      expect(
        parseFloat((c.querySelector("svg[role=img]") as SVGElement).style.width)
      ).toBeGreaterThan(w0 * 1.5);
      expect(boxes(c).length).toBeGreaterThanOrEqual(n0);
      const bs = boxes(c);
      for (let a = 0; a < bs.length; a++)
        for (let b = a + 1; b < bs.length; b++)
          expect(hit(bs[a], bs[b])).toBe(false);
    } finally {
      await root.unmount();
    }
  });
  it("full view uses the full layout and closes on Esc", async () => {
    const close = vi.fn(),
      root = createSmokeRoot();
    try {
      await root.render(
        createElement(JourneyStorylineFullscreen, {
          snapshot: snap,
          domain,
          titles,
          onSelect: vi.fn(),
          onClose: close,
        })
      );
      const d = document.querySelector(
        "[role=dialog][aria-label='Journey storyline full view']"
      )!;
      expect(d.querySelector("section")?.getAttribute("data-variant")).toBe(
        "full"
      );
      await dispatch(() => {
        window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
      });
      expect(close).toHaveBeenCalledTimes(1);
    } finally {
      await root.unmount();
    }
  });
});
