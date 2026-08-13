const ONE_MINUTE_MS = 60_000;

/** Average event rate across the full observation window shown by DevTools. */
export function ratePerMinuteInWindow(
  eventCount: number,
  windowMs: number
): number {
  if (eventCount <= 0 || windowMs <= 0) return 0;
  return eventCount * (ONE_MINUTE_MS / windowMs);
}

/**
 * Use only time that was actually observed during the panel's warm-up.
 * A one-second floor prevents a just-opened panel from reporting meaningless
 * hundreds-per-minute rates for a single mount-time request.
 */
export function effectiveObservationWindowMs(
  configuredWindowMs: number,
  observationStartedAtMs: number | null,
  nowMs: number,
  minimumWindowMs = 1_000
): number {
  if (configuredWindowMs <= 0) return 0;
  if (observationStartedAtMs === null) return configuredWindowMs;
  const observedMs = Math.max(0, nowMs - observationStartedAtMs);
  return Math.min(configuredWindowMs, Math.max(minimumWindowMs, observedMs));
}

/** A simultaneous fan-out batch is not, by itself, a polling loop. */
export function spansRepeatedActivity(
  firstTimestampMs: number,
  lastTimestampMs: number,
  minimumSpanMs = 1_000
): boolean {
  return lastTimestampMs - firstTimestampMs >= minimumSpanMs;
}
