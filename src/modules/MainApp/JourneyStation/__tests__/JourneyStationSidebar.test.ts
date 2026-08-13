import { describe, expect, it } from "vitest";

import {
  projectJourneySelection,
  sessionJourneySelection,
} from "../JourneyStationSidebar";

describe("JourneyStationSidebar scope identity", () => {
  it("uses the canonical project id rather than the mutable slug", () => {
    expect(
      projectJourneySelection({
        id: " proj-canonical ",
        slug: "display-only-slug",
        name: "PPCharge",
      })
    ).toEqual({ kind: "project", id: "proj-canonical", name: "PPCharge" });
  });

  it("does not create a project Journey selection from a slug alone", () => {
    expect(
      projectJourneySelection({
        id: "",
        slug: "legacy-display-slug",
        name: "Legacy project",
      })
    ).toBeNull();
  });

  it("keeps a canonical session id valid for the session list", () => {
    expect(sessionJourneySelection("session-123", "Recent session")).toEqual({
      kind: "session",
      id: "session-123",
      name: "Recent session",
    });
  });
});
