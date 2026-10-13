// @vitest-environment jsdom
import { act, createElement } from "react";
import { type Root, createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { CompactionPrefsView } from "@src/api/tauri/agent";

import CompactionPrefsCard from "./CompactionPrefsCard";

const api = vi.hoisted(() => ({
  getCompactionPrefs: vi.fn(),
  setCompactionThreshold: vi.fn(),
  setIdleCompaction: vi.fn(),
}));

vi.mock("@src/api/tauri/agent", () => api);
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      options ? `${key}|${JSON.stringify(options)}` : key,
  }),
}));
vi.mock("@src/util/time/scheduling/visibilityAwarePoller", () => ({
  startVisibilityAwarePoller: () => () => undefined,
}));
vi.mock("./useContextUsageInfo", () => ({
  formatTokenCount: (value: number) => String(value),
}));
vi.mock("@src/components/Switch", async () => {
  const ReactModule = await import("react");
  return {
    default: (props: {
      checked: boolean;
      disabled?: boolean;
      dataTestId?: string;
      onCheckedChange: (checked: boolean) => void;
    }) =>
      ReactModule.createElement("button", {
        "data-testid": props.dataTestId,
        "data-checked": String(props.checked),
        disabled: props.disabled,
        onClick: () => props.onCheckedChange(!props.checked),
      }),
  };
});
vi.mock("@src/components/NumberInput", async () => {
  const ReactModule = await import("react");
  return {
    default: (props: {
      value?: number;
      disabled?: boolean;
      dataTestId?: string;
      onValueChange?: (value: number | undefined) => void;
    }) =>
      ReactModule.createElement("input", {
        "data-testid": props.dataTestId,
        disabled: props.disabled,
        value: props.value ?? "",
        onChange: (event: { target: { value: string } }) =>
          props.onValueChange?.(Number(event.target.value)),
      }),
  };
});

const actEnvironment = globalThis as typeof globalThis & {
  IS_REACT_ACT_ENVIRONMENT?: boolean;
};

function makeView(
  overrides: Partial<CompactionPrefsView> = {},
  idle: Partial<CompactionPrefsView["idle"]> = {}
): CompactionPrefsView {
  return {
    sessionId: "s1",
    weightedThreshold: null,
    weightedUsedTokens: 1_250_000,
    agentDefaultThreshold: 5_000_000,
    agentDefaultFromRuntime: true,
    effectiveThreshold: 5_000_000,
    thresholdSource: "agent_default",
    thresholdMin: 10_000,
    thresholdMax: 1_000_000_000_000,
    idle: {
      supported: true,
      enabled: false,
      minutes: 59,
      minutesMin: 1,
      minutesMax: 1440,
      state: "disabled",
      lastAttempt: null,
      nowMs: 0,
      overdueGraceMs: 120_000,
      ...idle,
    },
    ...overrides,
  };
}

describe("CompactionPrefsCard", () => {
  let container: HTMLDivElement;
  let root: Root;

  const mount = async () => {
    await act(async () => {
      root.render(createElement(CompactionPrefsCard, { sessionId: "s1" }));
    });
  };
  const byId = (id: string) =>
    container.querySelector<HTMLElement>(`[data-testid="${id}"]`);
  const click = async (id: string) => {
    await act(async () => {
      byId(id)?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
  };

  beforeEach(() => {
    actEnvironment.IS_REACT_ACT_ENVIRONMENT = true;
    Object.values(api).forEach((mock) => mock.mockReset());
    api.getCompactionPrefs.mockResolvedValue(makeView());
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    Reflect.deleteProperty(actEnvironment, "IS_REACT_ACT_ENVIRONMENT");
  });

  it("shows live spend, the effective threshold and its source from the backend view", async () => {
    await mount();
    expect(api.getCompactionPrefs).toHaveBeenCalledWith("s1");
    expect(byId("context-info-compaction-spend-text")?.textContent).toContain(
      '"used":"1250000","limit":"5000000"'
    );
    expect(byId("context-info-compaction-threshold-source")?.textContent).toBe(
      "contextInfo.compactionSourceDefault"
    );
    expect(
      (byId("context-info-compaction-threshold-input") as HTMLInputElement)
        .value
    ).toBe("5");
    expect(byId("context-info-compaction-spend-bar")?.style.width).toBe("25%");
    expect(byId("context-info-compaction-idle-switch")?.dataset.checked).toBe(
      "false"
    );
  });

  it("a preset persists through the RPC and the UI adopts the returned view", async () => {
    api.setCompactionThreshold.mockResolvedValue(
      makeView({
        weightedThreshold: 10_000_000,
        effectiveThreshold: 10_000_000,
        thresholdSource: "session",
      })
    );
    await mount();
    await click("context-info-compaction-preset-10");
    expect(api.setCompactionThreshold).toHaveBeenCalledWith("s1", 10_000_000);
    expect(byId("context-info-compaction-threshold-source")?.textContent).toBe(
      "contextInfo.compactionSourceSession"
    );
    expect(byId("context-info-compaction-spend-bar")?.style.width).toBe(
      "12.5%"
    );
  });

  it("applies an edited value in tokens and keeps Apply disabled until it differs", async () => {
    api.setCompactionThreshold.mockResolvedValue(
      makeView({
        weightedThreshold: 3_000_000,
        effectiveThreshold: 3_000_000,
        thresholdSource: "session",
      })
    );
    await mount();
    expect(
      (byId("context-info-compaction-threshold-apply") as HTMLButtonElement)
        .disabled
    ).toBe(true);
    const input = byId(
      "context-info-compaction-threshold-input"
    ) as HTMLInputElement;
    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(
        HTMLInputElement.prototype,
        "value"
      )?.set;
      setter?.call(input, "3");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await click("context-info-compaction-threshold-apply");
    expect(api.setCompactionThreshold).toHaveBeenCalledWith("s1", 3_000_000);
  });

  it("turn-off sends 0 and reset-to-default sends null", async () => {
    api.setCompactionThreshold.mockResolvedValueOnce(
      makeView({
        weightedThreshold: 0,
        effectiveThreshold: 0,
        thresholdSource: "session",
      })
    );
    await mount();
    await click("context-info-compaction-spend-off");
    expect(api.setCompactionThreshold).toHaveBeenLastCalledWith("s1", 0);
    expect(byId("context-info-compaction-spend-text")?.textContent).toContain(
      "compactionSpendOff"
    );
    api.setCompactionThreshold.mockResolvedValueOnce(makeView());
    await click("context-info-compaction-threshold-reset");
    expect(api.setCompactionThreshold).toHaveBeenLastCalledWith("s1", null);
  });

  it("the idle switch sends the current interval and reflects the returned state", async () => {
    api.setIdleCompaction.mockResolvedValue(
      makeView({}, { enabled: true, state: "waiting_activity" })
    );
    await mount();
    await click("context-info-compaction-idle-switch");
    expect(api.setIdleCompaction).toHaveBeenCalledWith("s1", true, 59);
    expect(byId("context-info-compaction-idle-switch")?.dataset.checked).toBe(
      "true"
    );
    expect(byId("context-info-compaction-idle-state")?.textContent).toContain(
      "compactionIdleState.waiting_activity"
    );
  });

  it("unsupported sessions cannot be armed and show the backend reason", async () => {
    api.getCompactionPrefs.mockResolvedValue(
      makeView(
        {},
        {
          supported: false,
          unsupportedReason: "Channel-attached sessions compact by forking",
          state: "unsupported",
        }
      )
    );
    await mount();
    expect(
      (byId("context-info-compaction-idle-switch") as HTMLButtonElement)
        .disabled
    ).toBe(true);
    expect(byId("context-info-compaction-idle-unsupported")?.textContent).toBe(
      "Channel-attached sessions compact by forking"
    );
  });

  it("shows the last idle attempt and the cost note, and surfaces RPC errors", async () => {
    api.getCompactionPrefs.mockResolvedValue(
      makeView(
        {},
        {
          enabled: true,
          state: "attempted",
          lastAttempt: {
            epochMs: 1,
            atMs: 2,
            status: "skipped_overdue",
            tokensBefore: null,
            tokensAfter: null,
          },
        }
      )
    );
    api.setCompactionThreshold.mockRejectedValue(new Error("out of range"));
    await mount();
    expect(
      byId("context-info-compaction-idle-last-attempt")?.textContent
    ).toContain("compactionIdleStatus.skipped_overdue");
    expect(container.textContent).toContain("contextInfo.compactionIdleCost");
    await click("context-info-compaction-preset-1");
    expect(byId("context-info-compaction-prefs-error")?.textContent).toContain(
      "out of range"
    );
  });

  it("renders only an error when the first load fails", async () => {
    api.getCompactionPrefs.mockRejectedValue(new Error("db locked"));
    await mount();
    expect(byId("context-info-compaction-prefs")).toBeNull();
    expect(byId("context-info-compaction-prefs-error")?.textContent).toContain(
      "db locked"
    );
  });
});
