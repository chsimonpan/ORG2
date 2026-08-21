// @vitest-environment jsdom
import React, { act } from "react";
import { type Root, createRoot } from "react-dom/client";
import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from "vitest";

import TurnPaginationControls, {
  canNavigateTurnPage,
  shouldShowTurnPaginationSpinner,
} from "../TurnPaginationControls";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

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

describe("TurnPaginationControls DOM interaction", () => {
  let container: HTMLDivElement;
  let root: Root;
  const reactActEnvironment = globalThis as typeof globalThis & {
    IS_REACT_ACT_ENVIRONMENT?: boolean;
  };

  beforeAll(() => {
    reactActEnvironment.IS_REACT_ACT_ENVIRONMENT = true;
  });

  beforeEach(() => {
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    vi.clearAllMocks();
  });

  afterAll(() => {
    Reflect.deleteProperty(reactActEnvironment, "IS_REACT_ACT_ENVIRONMENT");
  });

  it.each([false, true])(
    "keeps the previous-round DOM button clickable when ready=%s",
    async (turnPaginationReady) => {
      const onPreviousTurnPage = vi.fn();
      const onNextTurnPage = vi.fn();
      const onLastTurnPage = vi.fn();

      await act(async () => {
        root.render(
          <TurnPaginationControls
            agentOrgOverviewOpen={false}
            setAgentOrgOverviewOpen={vi.fn()}
            turnPaginationEnabled
            turnPaginationReady={turnPaginationReady}
            turnPageListOpen={false}
            setTurnPageListOpen={vi.fn()}
            turnPageSortAscending
            setTurnPageSortAscending={vi.fn()}
            currentTurnPageLabel="3 / 3"
            currentTurnPageTimeLabel="now"
            currentPageIndex={2}
            pageCount={3}
            onPreviousTurnPage={onPreviousTurnPage}
            onNextTurnPage={onNextTurnPage}
            onLastTurnPage={onLastTurnPage}
          />
        );
      });

      const previous = container.querySelector<HTMLButtonElement>(
        '[data-testid="turn-pagination-previous-round"]'
      );
      const next = container.querySelector<HTMLButtonElement>(
        '[data-testid="turn-pagination-next-round"]'
      );
      const latest = container.querySelector<HTMLButtonElement>(
        '[data-testid="turn-pagination-last-round"]'
      );

      expect(previous).not.toBeNull();
      expect(previous?.disabled).toBe(false);
      expect(next?.disabled).toBe(true);
      expect(latest?.disabled).toBe(true);

      act(() => previous?.click());
      expect(onPreviousTurnPage).toHaveBeenCalledTimes(1);
    }
  );
});
