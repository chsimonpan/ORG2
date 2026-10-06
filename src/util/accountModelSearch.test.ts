import { describe, expect, it } from "vitest";

import { applyModelGroupToEnabledSet } from "@src/modules/MainApp/Integrations/KeyVault/Models/Table/integrationsModelGroups";

import { filterAccountModelGroups } from "./accountModelSearch";

const groups = [
  {
    label: "Claude Opus",
    sortVersion: 4,
    models: ["anthropic/opus:anthropic", "anthropic/opus:other"],
  },
  { label: "中文模型", sortVersion: 1, models: ["vendor/chinese"] },
];
describe("R6 account model search", () => {
  it.each([" OPUS ", "claude", "AnThRoPiC"])(
    "matches normalized ID/name %s",
    (query) => {
      expect(filterAccountModelGroups(groups, query)[0].models).toEqual(
        groups[0].models
      );
    }
  );
  it("matches Chinese labels and provider; empty restores complete source", () => {
    expect(filterAccountModelGroups(groups, "中文")[0].label).toBe("中文模型");
    expect(filterAccountModelGroups(groups, "ZenMux", "ZenMux")).toEqual(
      groups
    );
    expect(filterAccountModelGroups(groups, "  ")).toEqual(groups);
    expect(filterAccountModelGroups(groups, "missing")).toEqual([]);
  });
  it("keeps distinct supplier IDs and only toggles visible targets, preserving hidden enabled items", () => {
    const visible = filterAccountModelGroups(groups, ":anthropic");
    expect(visible[0].models).toEqual(["anthropic/opus:anthropic"]);
    const before = new Set(["vendor/chinese", "anthropic/opus:other"]);
    const all = groups.flatMap((g) => g.models);
    expect(
      new Set(applyModelGroupToEnabledSet(before, visible[0].models, all, true))
    ).toEqual(new Set([...before, "anthropic/opus:anthropic"]));
    expect(groups[0].models).toHaveLength(2);
  });
});
