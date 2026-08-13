import { describe, expect, it } from "vitest";

import {
  effectiveObservationWindowMs,
  ratePerMinuteInWindow,
  spansRepeatedActivity,
} from "../hotspotRates";

describe("hotspot rate helpers", () => {
  it("normalizes a batch against the displayed two-minute window", () => {
    expect(ratePerMinuteInWindow(10, 120_000)).toBe(5);
    expect(ratePerMinuteInWindow(1, 120_000)).toBe(0.5);
  });

  it("does not classify a near-simultaneous provider fan-out as polling", () => {
    expect(spansRepeatedActivity(10_000, 10_002)).toBe(false);
    expect(spansRepeatedActivity(10_000, 11_000)).toBe(true);
  });

  it("normalizes warm-up rates by elapsed observation time with a floor", () => {
    expect(effectiveObservationWindowMs(120_000, 10_000, 10_100)).toBe(1_000);
    expect(effectiveObservationWindowMs(120_000, 10_000, 40_000)).toBe(30_000);
    expect(effectiveObservationWindowMs(120_000, 10_000, 200_000)).toBe(
      120_000
    );
    expect(effectiveObservationWindowMs(120_000, null, 20_000)).toBe(120_000);
  });
});
