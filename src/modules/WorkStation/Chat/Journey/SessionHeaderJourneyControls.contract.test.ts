import { describe, expect, it } from "vitest";

import chatViewSource from "@src/engines/ChatPanel/ChatView.tsx?raw";

import communicationSource from "../Communication/index.tsx?raw";

describe("real Session header Journey controls", () => {
  it("mounts SessionJourneyControls in both production Session header paths", () => {
    expect(communicationSource).toContain("<SessionJourneyControls");
    expect(communicationSource).toContain("extraActions=");
    expect(chatViewSource).toContain(
      'data-testid="live-session-journey-controls"'
    );
    expect(chatViewSource).toContain("<SessionJourneyControls");
  });
});
