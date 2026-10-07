import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  addGlobalPathExemption,
  listGlobalPathExemptions,
  removeGlobalPathExemption,
} from "./globalPathExemptions";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

beforeEach(() => {
  invoke.mockReset();
  invoke.mockResolvedValue([]);
});
describe("grant management RPC contract", () => {
  it("lists with the registered command", async () => {
    await listGlobalPathExemptions();
    expect(invoke).toHaveBeenCalledWith("global_path_exemptions_list");
  });
  it("passes only explicit grant fields", async () => {
    await addGlobalPathExemption({
      path: "/fixture",
      mode: "read",
      recursive: false,
    });
    expect(invoke).toHaveBeenCalledWith("global_path_exemptions_add", {
      path: "/fixture",
      mode: "read",
      recursive: false,
    });
  });
  it("removes by persisted canonical identity", async () => {
    await removeGlobalPathExemption("/fixture");
    expect(invoke).toHaveBeenCalledWith("global_path_exemptions_remove", {
      path: "/fixture",
    });
  });
  it("propagates backend errors", async () => {
    invoke.mockRejectedValue(new Error("denied"));
    await expect(listGlobalPathExemptions()).rejects.toThrow("denied");
  });
  it("rejects malformed native success replies", async () => {
    invoke.mockResolvedValue([
      { path: "/fixture", mode: "all", recursive: true },
    ]);
    await expect(listGlobalPathExemptions()).rejects.toThrow();
  });
  it("does not dispatch invalid frontend grants", async () => {
    await expect(
      addGlobalPathExemption({ path: "bad\0", mode: "read", recursive: false })
    ).rejects.toThrow();
    expect(invoke).not.toHaveBeenCalled();
  });
});
