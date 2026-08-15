import { describe, expect, it } from "vitest";

import type { KeyVaultAccount } from "@src/hooks/keyVault/types";
import type { LastModelSelection } from "@src/store/session/creatorDefaultModelAtom";

import { resolveModelDisplaySelection } from "./resolveModelDisplaySelection";

function codexAccount(): KeyVaultAccount {
  return {
    id: "chatgpt-account-a",
    name: "ChatGPT A",
    modelType: "codex",
    status: "ready",
    enabled: true,
    hasLocalKey: true,
    isListed: false,
    hasKey: true,
    hasApiKey: false,
    hasSessionToken: true,
    availableModels: ["gpt-5.6-terra"],
    enabledModels: ["gpt-5.6-terra"],
    modelVariants: [
      {
        model: "gpt-5.6-terra-medium",
        base_model: "gpt-5.6-terra",
        reasoning: "medium",
        fast: false,
      },
      {
        model: "gpt-5.6-terra-ultra",
        base_model: "gpt-5.6-terra",
        reasoning: "ultra",
        fast: false,
      },
    ],
    defaultVariants: [
      {
        base_model: "gpt-5.6-terra",
        model: "gpt-5.6-terra-medium",
      },
    ],
  } as KeyVaultAccount;
}

function selection(model: string): LastModelSelection {
  return {
    keySource: "own_key",
    model,
    selectedAccountId: "chatgpt-account-a",
    selectedSourceModelType: "codex",
  };
}

describe("resolveModelDisplaySelection", () => {
  it("keeps the reasoning effort selected for the active session", () => {
    const selected = selection("gpt-5.6-terra-ultra");

    expect(resolveModelDisplaySelection(selected, [codexAccount()], true)).toBe(
      selected
    );
  });

  it("applies the account default only to a bare base-model selection", () => {
    expect(
      resolveModelDisplaySelection(
        selection("gpt-5.6-terra"),
        [codexAccount()],
        true
      )?.model
    ).toBe("gpt-5.6-terra-medium");
  });

  it("does not rewrite a historical session selection", () => {
    const selected = selection("gpt-5.6-terra");

    expect(
      resolveModelDisplaySelection(selected, [codexAccount()], false)
    ).toBe(selected);
  });
});
