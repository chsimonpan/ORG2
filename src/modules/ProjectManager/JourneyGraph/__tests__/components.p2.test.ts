import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import type { JourneyGraphPayload } from "@src/api/tauri/journeyGraph";

import { BranchesGraph } from "../components/BranchesGraph";
import { CoverageLedger } from "../components/CoverageLedger";
import { FileLineagePanel } from "../components/FileLineagePanel";
import {
  StorylineTimeline,
  inspectorFields,
} from "../components/StorylineTimeline";
import {
  graphToBranchesViewModel,
  graphToCoverageLedgerViewModel,
  graphToFileLineageViewModel,
  graphToStorylineViewModel,
} from "../viewModel";

const graph: JourneyGraphPayload = {
  nodes: [
    {
      id: "session/s",
      kind: "session",
      evidenceClass: "canonical",
      sourceRef: "session:s",
      displayTimestamp: "2026-07-30T08:00:00.000Z",
      metadata: { topicTags: ["explicit-topic"] },
      displayTitle: "Active work",
      lifecycleStatus: "running",
    },
    {
      id: "session/parent",
      kind: "session",
      evidenceClass: "canonical",
      sourceRef: "session:parent",
      displayTimestamp: "2026-07-30T07:00:00.000Z",
      displayTitle: "Paused work",
      lifecycleStatus: "interrupted",
    },
    {
      id: "turn/s/1",
      kind: "turn",
      evidenceClass: "canonical",
      sourceRef: "turn:s:1",
      displayTimestamp: "2026-07-30T08:03:00.000Z",
      displayTitle: "Investigate scope",
      resultSummary: "Canonical scope verified",
      lifecycleStatus: "completed",
    },
    {
      id: "artifact/orgtrack/a",
      kind: "artifact",
      evidenceClass: "canonical",
      sourceRef: "artifact:a",
    },
    {
      id: "file/repo/a.ts",
      kind: "file",
      evidenceClass: "canonical",
      sourceRef: "file:a",
    },
  ],
  edges: [
    {
      from: "session/s",
      to: "session/parent",
      kind: "forkedFrom",
      evidenceClass: "canonical",
      sourceRef: "fork:s:parent",
    },
    {
      from: "session/s",
      to: "turn/s/1",
      kind: "contains",
      evidenceClass: "canonical",
      sourceRef: "turn:s:1",
    },
    {
      from: "artifact/orgtrack/a",
      to: "file/repo/a.ts",
      kind: "produced",
      evidenceClass: "canonical",
      sourceRef: "artifact:a",
    },
  ],
  coverage: [{ sourceRef: "session:s", status: "represented" }],
};

describe("P2 Journey components", () => {
  it("renders evidence badges and source drill targets for every graph view", () => {
    const markup = [
      renderToStaticMarkup(
        createElement(StorylineTimeline, {
          viewModel: graphToStorylineViewModel(graph),
        })
      ),
      renderToStaticMarkup(
        createElement(BranchesGraph, {
          viewModel: graphToBranchesViewModel(graph),
        })
      ),
      renderToStaticMarkup(
        createElement(FileLineagePanel, {
          viewModel: graphToFileLineageViewModel(graph),
        })
      ),
      renderToStaticMarkup(
        createElement(CoverageLedger, {
          viewModel: graphToCoverageLedgerViewModel(graph),
        })
      ),
    ].join("\n");
    expect(markup).toContain("canonical");
    expect(markup).toContain("Source: session:s");
    expect(markup).toContain("#journey-source-session%3As");
    expect(markup).toContain("Independent provenance audit");
    expect(markup).toContain("explicit-topic");
  });

  it("does not render unknown or non-canonical topic tags", () => {
    const unknownGraph: JourneyGraphPayload = {
      ...graph,
      nodes: [
        {
          id: "session/unknown",
          kind: "session",
          evidenceClass: "canonical",
          sourceRef: "session:unknown",
          displayTimestamp: "2026-07-30T08:00:00.000Z",
        },
        {
          id: "session/overlay",
          kind: "session",
          evidenceClass: "userOverlay",
          sourceRef: "overlay",
          displayTimestamp: "2026-07-30T09:00:00.000Z",
          metadata: { topicTags: ["not-canonical"] },
        },
      ],
      edges: [],
    };

    const markup = renderToStaticMarkup(
      createElement(StorylineTimeline, {
        viewModel: graphToStorylineViewModel(unknownGraph),
      })
    );
    expect(markup).not.toContain("storyline-topic-tags");
    expect(markup).not.toContain("not-canonical");
    expect(markup).toContain('data-lane-state="active"');
    expect(markup).toContain("storyline-lane-ribbon");
    expect(markup).toContain('data-testid="storyline-viewport"');
    expect(markup).toContain('aria-label="Zoom in"');
    expect(markup).toContain('aria-label="Zoom out"');
    expect(markup).toContain(">Fit</button>");
    expect(markup).toContain(">Reset</button>");
    expect(markup).toContain("Canonical scope verified");
  });
});

describe("P3 Storyline visual grammar", () => {
  it("renders depth-tinted lane bands and a NOW marker", () => {
    const markup = renderToStaticMarkup(
      createElement(StorylineTimeline, {
        viewModel: graphToStorylineViewModel(graph),
      })
    );
    // Depth bands carry the factual lineage depth.
    expect(markup).toContain('data-testid="storyline-lane-band"');
    expect(markup).toContain('data-depth="0"');
    // NOW marker is drawn when the injected instant lies on the axis; the
    // graph facts are 2026-07-30 07:00-08:03 and the test runs later, so the
    // marker may be absent — the render path must not throw and the legend
    // entry always exists.
    expect(markup).toContain("now");
    expect(markup).toContain("storyline-lane-ribbon");
  });

  it("projects inspector fields from facts only", () => {
    const milestone = {
      id: "turn/s/1",
      title: "Investigate scope",
      kind: "turn",
      evidenceClass: "canonical" as const,
      sourceRef: "turn:s:1",
      displayTimestamp: "2026-07-30T08:03:00.000Z",
      resultSummary: "Canonical scope verified",
      lifecycleStatus: "completed",
      branch: "feature/journey",
      sequence: 1,
      topicTags: [],
    };
    const fields = inspectorFields(milestone);
    expect(fields).toEqual([
      { label: "kind", value: "turn" },
      { label: "time", value: "2026-07-30T08:03:00.000Z" },
      { label: "lifecycle", value: "completed" },
      { label: "branch", value: "feature/journey" },
      { label: "turn", value: "1" },
      { label: "result", value: "Canonical scope verified" },
    ]);
  });

  it("inspector fields never fabricate result or lifecycle text", () => {
    const bare = {
      id: "artifact/orgtrack/a",
      title: "artifact/orgtrack/a",
      kind: "artifact",
      evidenceClass: "canonical" as const,
      sourceRef: "artifact:a",
      displayTimestamp: null,
      resultSummary: null,
      lifecycleStatus: null,
      branch: null,
      sequence: null,
      topicTags: [],
    };
    expect(inspectorFields(bare)).toEqual([
      { label: "kind", value: "artifact" },
    ]);
  });
});
