/**
 * Journal instrumentation on the pass boundary.
 *
 * The load-bearing case is the NEGATIVE one: a successful pass advances the
 * last-sync stamp but must not consume a journal slot. Passes are
 * activity-debounced (~1.5-3s), so journaling every success would evict the
 * bounded ring's warnings within minutes of active work — precisely when the
 * Sync tab's log is worth reading.
 */
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import {
  cleanupEngineFixture,
  createEngineFixture,
} from "./org2CloudSyncEngine.testUtils";
import type { EngineFixture } from "./org2CloudSyncEngine.testUtils";
import {
  getLastSyncState,
  getSyncJournalSnapshot,
  resetSyncJournalForTests,
} from "./org2CloudSyncJournal";

describe("Org2CloudSyncEngine journal instrumentation", () => {
  let fixture: EngineFixture;
  let engine: EngineFixture["engine"];

  beforeEach(() => {
    resetSyncJournalForTests();
    fixture = createEngineFixture();
    ({ engine } = fixture);
  });

  afterEach(() => {
    cleanupEngineFixture(engine);
    resetSyncJournalForTests();
  });

  it("advances the last-sync stamp without journaling a successful pass", async () => {
    await engine.runSyncPass();

    expect(getSyncJournalSnapshot()).toEqual([]);
    const { lastSuccessAtMs, lastPassAtMs } = getLastSyncState();
    expect(lastSuccessAtMs).toBeGreaterThan(0);
    expect(lastPassAtMs).toBeGreaterThan(0);
  });

  it("keeps the log free of success noise across repeated passes", async () => {
    await engine.runSyncPass();
    await engine.runSyncPass();
    await engine.runSyncPass();

    expect(getSyncJournalSnapshot()).toHaveLength(0);
  });
});
