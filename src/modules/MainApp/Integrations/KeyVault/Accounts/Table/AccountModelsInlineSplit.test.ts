// @vitest-environment jsdom
import React, { act } from "react";
import { type Root, createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import type { KeyVaultAccount } from "@src/hooks/keyVault";

import AccountModelsInlineSplit from "./AccountModelsInlineSplit";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (s: string) => s }),
}));
vi.mock("@src/components/AnyIcon", () => ({ default: () => null }));
vi.mock("@src/components/ModelIcon", () => ({ default: () => null }));
vi.mock("@src/components/Tooltip", () => ({
  default: ({ children }: { children: React.ReactNode }) => children,
}));
vi.mock("@src/components/Button", () => ({
  default: ({
    children,
    onClick,
  }: {
    children: React.ReactNode;
    onClick?: () => void;
  }) => React.createElement("button", { onClick }, children),
}));
vi.mock("@src/components/Switch", () => ({ default: () => null }));
vi.mock(
  "@src/modules/MainApp/Integrations/KeyVault/shared/InlineCardPrimitives",
  () => ({
    InlineCardScrollList: ({ children }: { children: React.ReactNode }) =>
      React.createElement("div", null, children),
  })
);
vi.mock(
  "@src/modules/MainApp/Integrations/KeyVault/shared/InlineSplitRows",
  () => ({
    InlineSplitHeaderRow: () => React.createElement("div", null, "All models"),
    InlineSplitKeyRow: ({
      label,
      onToggle,
      switchChecked,
    }: {
      label: React.ReactNode;
      onToggle: (value: boolean) => void;
      switchChecked: boolean;
    }) =>
      React.createElement(
        "div",
        { "data-testid": "model-row" },
        label,
        React.createElement(
          "button",
          { onClick: () => onToggle(!switchChecked) },
          "Toggle"
        )
      ),
  })
);
vi.mock(
  "@src/modules/MainApp/Integrations/KeyVault/shared/ModelTable/ModelVariantInlineCard",
  () => ({ default: () => null })
);
let root: Root;
let host: HTMLDivElement;
const update = vi.fn();
const account = {
  id: "A",
  name: "ZenMux",
  enabled: true,
  modelType: "custom",
  availableModels: ["anthropic/opus:anthropic", "openai/gpt-5.5"],
  defaultVariants: [],
} as unknown as KeyVaultAccount;
const hiddenEnabled = new Set(["openai/gpt-5.5"]);
function render(a = account) {
  act(() =>
    root.render(
      React.createElement(AccountModelsInlineSplit, {
        account: a,
        enabledSet: hiddenEnabled,
        isAccountEnabled: a.enabled,
        variantsByModel: new Map(),
        onSetModelEnabled: () => undefined,
        onUpdateEnabledModels: update,
      })
    )
  );
}
function query(value: string) {
  const input = host.querySelector("input")!;
  act(() => {
    Object.getOwnPropertyDescriptor(
      HTMLInputElement.prototype,
      "value"
    )!.set!.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
beforeEach(() => {
  (
    globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }
  ).IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  update.mockClear();
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  delete (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean })
    .IS_REACT_ACT_ENVIRONMENT;
});
it("R6-I01 filters rendered groups without losing hidden enabled IDs on toggle", () => {
  render();
  query(":anthropic");
  expect(host.querySelectorAll("[data-testid=model-row]")).toHaveLength(1);
  act(() =>
    (
      host.querySelector("[data-testid=model-row] button") as HTMLButtonElement
    ).click()
  );
  expect(new Set(update.mock.calls[0][0])).toEqual(
    new Set(["openai/gpt-5.5", "anthropic/opus:anthropic"])
  );
  query("");
  expect(host.querySelectorAll("[data-testid=model-row]")).toHaveLength(2);
});
it("R6-I01 clears account query and never writes model/account settings during search", () => {
  render();
  query("missing");
  expect(host.textContent).toContain("No matching models");
  expect(update).not.toHaveBeenCalled();
  render({ ...account, id: "B", enabled: false });
  expect(host.querySelector("input")!.value).toBe("");
  expect(update).not.toHaveBeenCalled();
});
