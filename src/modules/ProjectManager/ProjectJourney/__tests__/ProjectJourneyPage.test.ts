import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import ProjectJourneyPage from "../ProjectJourneyPage";

describe("ProjectJourneyPage", () => {
  it("requires a canonical project id and never falls back to a mutable slug", () => {
    const markup = renderToStaticMarkup(
      createElement(ProjectJourneyPage, {
        projectSlug: "legacy-display-slug",
        projectName: "Legacy project",
      })
    );

    expect(markup).toContain("Project identity is required");
    expect(markup).not.toContain("journey-container");
  });

  it("opens the shared canonical Journey container for an explicit project id", () => {
    const markup = renderToStaticMarkup(
      createElement(ProjectJourneyPage, {
        projectId: "proj-123",
        projectSlug: "display-only-slug",
        projectName: "Canonical project",
      })
    );

    expect(markup).toContain("journey-container");
    expect(markup).toContain("Project Journey · Canonical project");
  });
});
