import { describe, expect, it } from "vitest";

import {
  CompactionPrefsSetIdleInput,
  CompactionPrefsSetThresholdInput,
  CompactionPrefsViewSchema,
  ManualCompactResultSchema,
} from "./agentSession";

// Exact shape produced by the Rust `CompactionPrefsView` (camelCase, optional
// fields skipped, Option -> null).
const wire = {
  sessionId: "s1",
  weightedThreshold: null,
  weightedUsedTokens: 12,
  agentDefaultThreshold: 5_000_000,
  agentDefaultFromRuntime: false,
  effectiveThreshold: 5_000_000,
  thresholdSource: "agent_default",
  thresholdMin: 10_000,
  thresholdMax: 1_000_000_000_000,
  idle: {
    supported: true,
    enabled: true,
    minutes: 59,
    minutesMin: 1,
    minutesMax: 1440,
    state: "waiting",
    nextDueMs: 99,
    lastAttempt: {
      epochMs: 1,
      atMs: 2,
      status: "compacted",
      tokensBefore: 10,
      tokensAfter: null,
    },
    nowMs: 5,
    overdueGraceMs: 120_000,
  },
};

describe("compaction prefs wire schemas", () => {
  it("accepts the Rust view including nulls and skipped optional fields", () => {
    const parsed = CompactionPrefsViewSchema.parse(wire);
    expect(parsed.weightedThreshold).toBeNull();
    expect(parsed.idle.unsupportedReason).toBeUndefined();
    expect(parsed.idle.lastAttempt?.tokensAfter).toBeNull();
  });

  it("accepts lastAttempt: null (a session that never attempted) and rejects an omitted key", () => {
    const { lastAttempt: _drop, ...idleWithout } = wire.idle;
    expect(
      CompactionPrefsViewSchema.parse({
        ...wire,
        idle: { ...idleWithout, lastAttempt: null },
      }).idle.lastAttempt
    ).toBeNull();
    expect(() =>
      CompactionPrefsViewSchema.parse({ ...wire, idle: idleWithout })
    ).toThrow();
  });

  it("rejects unknown states so a backend/UI drift fails loudly", () => {
    expect(() =>
      CompactionPrefsViewSchema.parse({
        ...wire,
        idle: { ...wire.idle, state: "sleeping" },
      })
    ).toThrow();
  });

  it("threshold input allows null and 0 but not negatives or fractions", () => {
    expect(
      CompactionPrefsSetThresholdInput.parse({
        sessionId: "s",
        threshold: null,
      })
    ).toBeTruthy();
    expect(
      CompactionPrefsSetThresholdInput.parse({ sessionId: "s", threshold: 0 })
    ).toBeTruthy();
    expect(() =>
      CompactionPrefsSetThresholdInput.parse({ sessionId: "s", threshold: -1 })
    ).toThrow();
    expect(() =>
      CompactionPrefsSetThresholdInput.parse({ sessionId: "s", threshold: 1.5 })
    ).toThrow();
  });

  it("idle input requires a positive integer interval", () => {
    expect(
      CompactionPrefsSetIdleInput.parse({
        sessionId: "s",
        enabled: true,
        minutes: 59,
      })
    ).toBeTruthy();
    expect(() =>
      CompactionPrefsSetIdleInput.parse({
        sessionId: "s",
        enabled: true,
        minutes: 0,
      })
    ).toThrow();
  });

  it("the manual compaction result accepts the idle-only skipped status", () => {
    expect(ManualCompactResultSchema.parse({ status: "skipped" }).status).toBe(
      "skipped"
    );
  });
});
