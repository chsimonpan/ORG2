// @vitest-environment jsdom
import { invoke } from "@tauri-apps/api/core";
import { createElement } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { createSmokeRoot, dispatch, settle } from "@src/test/reactSmokeHarness";

import { JourneyReviewQueue } from "./JourneyReviewQueue";
import type { JourneyDomainSnapshot, ReviewJobView } from "./nativeAdapter";
import type { DurableAnchor } from "./types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const job = (over: Partial<ReviewJobView> = {}): ReviewJobView => ({
  job_id: "jv-review:r1",
  review_id: "r1",
  fork_id: "fork",
  session_id: "A",
  state: "queued",
  attempt: 0,
  error: null,
  model_id: "anthropic/claude-sonnet-5.5",
  created_at: "2026-10-14T00:00:00Z",
  started_at: null,
  finished_at: null,
  ...over,
});
const domain = (
  over: Partial<JourneyDomainSnapshot["snapshot"]> = {}
): JourneyDomainSnapshot => ({
  revision: 7,
  snapshot: {
    session_id: "A",
    active_branch_id: "fork",
    tasks: {},
    branches: {
      main: { id: "main", name: "main", parent_branch_id: "", state: "active" },
      fork: {
        id: "fork",
        name: "fork",
        parent_branch_id: "main",
        parent_anchor_message_id: "anchor",
        state: "active",
      },
    },
    checkpoints: {},
    facts: {},
    reviews: {},
    ...over,
  },
});
// Real canonical anchors always carry branchId "session:<owner>", never a Journey fork id.
const anchor = (over: Partial<DurableAnchor> = {}): DurableAnchor =>
  ({
    ownerSessionId: "A",
    sourceSessionId: "A",
    branchId: "session:A",
    durableMessageId: "m5",
    boundaryVersion: 1,
    boundaryHash: "h",
    ...over,
  }) as DurableAnchor;
const buttons = (root: { container: HTMLElement }) =>
  Array.from(root.container.querySelectorAll("button"));
const button = (root: { container: HTMLElement }, text: string) =>
  buttons(root).find((b) => b.textContent?.includes(text));

let jobs: ReviewJobView[] = [];
beforeEach(() => {
  vi.useFakeTimers();
  jobs = [];
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (cmd: string) => {
    if (cmd === "journey_review_jobs") return jobs;
    return jobs[0];
  });
});
afterEach(() => vi.useRealTimers());

it("closes the active fork at the exact selected endpoint with a fresh review id and CAS revision", async () => {
  const root = createSmokeRoot();
  const changed = vi.fn();
  try {
    await root.render(
      createElement(JourneyReviewQueue, {
        ownerSessionId: "A",
        domain: domain(),
        anchor: anchor(),
        onChanged: changed,
      })
    );
    await settle(0);
    jobs = [job()];
    await dispatch(() => button(root, "Close fork for review")!.click());
    const call = vi
      .mocked(invoke)
      .mock.calls.find(([cmd]) => cmd === "journey_fork_close")!;
    expect(call[1]).toMatchObject({
      ownerSessionId: "A",
      request: {
        sessionId: "A",
        expectedRevision: 7,
        forkId: "fork",
        outcome: "completed",
        messageId: "m5",
      },
    });
    expect(
      (call[1] as { request: { reviewId: string } }).request.reviewId
    ).toMatch(/.{8,}/);
    expect(changed).toHaveBeenCalled();
    expect(root.container.textContent).toContain(
      "queued / attempt 0 / model anthropic/claude-sonnet-5.5"
    );
  } finally {
    await root.unmount();
  }
});

it("close is disabled without an endpoint on the active fork, on the main branch, or for a foreign owner anchor", async () => {
  const root = createSmokeRoot();
  try {
    await root.render(
      createElement(JourneyReviewQueue, {
        ownerSessionId: "A",
        domain: domain(),
        anchor: null,
        onChanged: vi.fn(),
      })
    );
    expect(button(root, "Close fork for review")!.disabled).toBe(true);
    await root.render(
      createElement(JourneyReviewQueue, {
        ownerSessionId: "A",
        domain: domain(),
        anchor: anchor({ sourceSessionId: "B" }),
        onChanged: vi.fn(),
      })
    );
    expect(button(root, "Close fork for review")!.disabled).toBe(true);
    await root.render(
      createElement(JourneyReviewQueue, {
        ownerSessionId: "A",
        domain: domain({ active_branch_id: "main" }),
        anchor: anchor(),
        onChanged: vi.fn(),
      })
    );
    expect(button(root, "Close fork for review")!.disabled).toBe(true);
    await root.render(
      createElement(JourneyReviewQueue, {
        ownerSessionId: "B",
        domain: domain(),
        anchor: anchor(),
        onChanged: vi.fn(),
      })
    );
    expect(button(root, "Close fork for review")!.disabled).toBe(true);
    expect(
      vi.mocked(invoke).mock.calls.some(([cmd]) => cmd === "journey_fork_close")
    ).toBe(false);
  } finally {
    await root.unmount();
  }
});

it("polls every 3 seconds only while a job is queued or running and stops on a terminal state", async () => {
  const root = createSmokeRoot();
  const changed = vi.fn();
  const listCalls = () =>
    vi
      .mocked(invoke)
      .mock.calls.filter(([cmd]) => cmd === "journey_review_jobs").length;
  try {
    jobs = [job({ state: "running", attempt: 1 })];
    await root.render(
      createElement(JourneyReviewQueue, {
        ownerSessionId: "A",
        domain: domain(),
        anchor: null,
        onChanged: changed,
      })
    );
    await settle(0);
    const initial = listCalls();
    await settle(3000);
    await settle(3000);
    expect(listCalls()).toBe(initial + 2);
    jobs = [job({ state: "ready", attempt: 1 })];
    await settle(3000);
    expect(changed).toHaveBeenCalledTimes(1);
    const afterTerminal = listCalls();
    await settle(12000);
    expect(listCalls()).toBe(afterTerminal);
    expect(root.container.textContent).toContain("ready / attempt 1");
  } finally {
    await root.unmount();
  }
});

it("does not poll while the document is hidden", async () => {
  const root = createSmokeRoot();
  const visibility = vi
    .spyOn(document, "visibilityState", "get")
    .mockReturnValue("hidden");
  try {
    jobs = [job({ state: "queued" })];
    await root.render(
      createElement(JourneyReviewQueue, {
        ownerSessionId: "A",
        domain: domain(),
        anchor: null,
        onChanged: vi.fn(),
      })
    );
    await settle(0);
    const initial = vi.mocked(invoke).mock.calls.length;
    await settle(9000);
    expect(vi.mocked(invoke).mock.calls.length).toBe(initial);
  } finally {
    visibility.mockRestore();
    await root.unmount();
  }
});

it("cancels a queued or running job by exact job id, and offers no cancel for terminal jobs", async () => {
  const root = createSmokeRoot();
  try {
    jobs = [
      job({ state: "running", attempt: 1 }),
      job({
        job_id: "jv-review:r2",
        review_id: "r2",
        state: "failed",
        error: "Provider resolution failed: exact provenance mismatch",
      }),
    ];
    await root.render(
      createElement(JourneyReviewQueue, {
        ownerSessionId: "A",
        domain: domain(),
        anchor: null,
        onChanged: vi.fn(),
      })
    );
    await settle(0);
    expect(button(root, "Cancel review r2")).toBeUndefined();
    expect(root.container.textContent).toContain(
      "Provider resolution failed: exact provenance mismatch"
    );
    await dispatch(() => button(root, "Cancel review r1")!.click());
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("journey_review_cancel", {
      ownerSessionId: "A",
      request: { sessionId: "A", jobId: "jv-review:r1" },
    });
  } finally {
    await root.unmount();
  }
});

it("shows a cancelled job with its visible reason and a command error as an alert", async () => {
  const root = createSmokeRoot();
  try {
    jobs = [
      job({
        state: "cancelled",
        error: "Cancelled by user. Retry to run it again.",
      }),
    ];
    await root.render(
      createElement(JourneyReviewQueue, {
        ownerSessionId: "A",
        domain: domain(),
        anchor: null,
        onChanged: vi.fn(),
      })
    );
    await settle(0);
    expect(root.container.textContent).toContain("cancelled / attempt 0");
    expect(root.container.textContent).toContain("Cancelled by user");
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "journey_review_jobs") return jobs;
      throw new Error("revision conflict");
    });
    await root.render(
      createElement(JourneyReviewQueue, {
        ownerSessionId: "A",
        domain: domain(),
        anchor: anchor(),
        onChanged: vi.fn(),
      })
    );
    await dispatch(() => button(root, "Close fork for review")!.click());
    expect(root.container.querySelector("[role=alert]")?.textContent).toContain(
      "revision conflict"
    );
  } finally {
    await root.unmount();
  }
});

it("rejects a job list that belongs to a different owner", async () => {
  const root = createSmokeRoot();
  try {
    jobs = [job({ session_id: "OTHER" })];
    await root.render(
      createElement(JourneyReviewQueue, {
        ownerSessionId: "A",
        domain: domain(),
        anchor: null,
        onChanged: vi.fn(),
      })
    );
    await settle(0);
    expect(root.container.textContent).not.toContain("jv-review");
    expect(root.container.querySelector("[role=alert]")?.textContent).toContain(
      "mismatch"
    );
  } finally {
    await root.unmount();
  }
});

it("clears a transient poll error once the next refresh succeeds", async () => {
  const root = createSmokeRoot();
  try {
    jobs = [job({ state: "running", attempt: 1 })];
    await root.render(
      createElement(JourneyReviewQueue, {
        ownerSessionId: "A",
        domain: domain(),
        anchor: null,
        onChanged: vi.fn(),
      })
    );
    await settle(0);
    let failing = true;
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "journey_review_jobs") {
        if (failing) throw new Error("registry busy");
        return jobs;
      }
      return jobs[0];
    });
    await settle(3000);
    expect(root.container.querySelector("[role=alert]")?.textContent).toContain(
      "registry busy"
    );
    failing = false;
    await settle(3000);
    expect(root.container.querySelector("[role=alert]")).toBeNull();
  } finally {
    await root.unmount();
  }
});
