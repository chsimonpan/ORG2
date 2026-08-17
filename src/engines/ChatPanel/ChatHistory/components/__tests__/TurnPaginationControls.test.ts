import { describe, expect, it } from "vitest";

import {
  canNavigateTurnPage,
  shouldShowTurnPaginationSpinner,
} from "../TurnPaginationControls";

describe("shouldShowTurnPaginationSpinner", () => {
  it("does not animate a stable empty session", () => {
    expect(
      shouldShowTurnPaginationSpinner({
        turnPaginationReady: false,
        pageCount: 0,
      })
    ).toBe(false);
  });

  it("animates while an existing round is still hydrating", () => {
    expect(
      shouldShowTurnPaginationSpinner({
        turnPaginationReady: false,
        pageCount: 1,
      })
    ).toBe(true);
  });

  it("stops once the current round is ready", () => {
    expect(
      shouldShowTurnPaginationSpinner({
        turnPaginationReady: true,
        pageCount: 1,
      })
    ).toBe(false);
  });
});

describe("canNavigateTurnPage", () => {
  it("keeps both arrows actionable while a known page is hydrating", () => {
    expect(
      canNavigateTurnPage({
        direction: "previous",
        currentPageIndex: 1,
        pageCount: 3,
      })
    ).toBe(true);
    expect(
      canNavigateTurnPage({
        direction: "next",
        currentPageIndex: 1,
        pageCount: 3,
      })
    ).toBe(true);
  });

  it("only disables at the actual history boundaries", () => {
    expect(
      canNavigateTurnPage({
        direction: "previous",
        currentPageIndex: 0,
        pageCount: 3,
      })
    ).toBe(false);
    expect(
      canNavigateTurnPage({
        direction: "next",
        currentPageIndex: 2,
        pageCount: 3,
      })
    ).toBe(false);
  });
});
