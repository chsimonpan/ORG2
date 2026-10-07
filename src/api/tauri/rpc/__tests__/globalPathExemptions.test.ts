import { describe, expect, it } from "vitest";

import {
  AddInput,
  EntrySchema,
  RemoveInput,
} from "../schemas/globalPathExemptions";

describe("global path grant schemas fail closed", () => {
  it("requires explicit capability and recursion", () => {
    expect(AddInput.safeParse({ path: "/fixture" }).success).toBe(false);
    expect(
      AddInput.safeParse({ path: "/fixture", mode: "read", recursive: false })
        .success
    ).toBe(true);
  });
  it("rejects invented modes, NULs, and session authority fields", () => {
    for (const value of [
      { path: "/fixture", mode: "all", recursive: true },
      { path: "bad\0", mode: "read", recursive: false },
      { path: "/fixture", mode: "read", recursive: false, sessionId: "other" },
    ]) {
      expect(AddInput.safeParse(value).success).toBe(false);
    }
  });
  it("remove uses persisted identity only", () => {
    expect(
      RemoveInput.safeParse({ path: "/fixture", recursive: true }).success
    ).toBe(false);
    expect(RemoveInput.safeParse({ path: "/fixture" }).success).toBe(true);
  });
  it("rejects malformed native records", () => {
    expect(
      EntrySchema.safeParse({
        path: "/fixture",
        mode: "read_write",
        recursive: "true",
      }).success
    ).toBe(false);
  });
});
