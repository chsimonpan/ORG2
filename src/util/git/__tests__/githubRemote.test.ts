import { describe, expect, it } from "vitest";

import {
  parseRepoFullNameFromRemote,
  resolveGithubRepoFullName,
} from "@src/util/git/githubRemote";

describe("parseRepoFullNameFromRemote", () => {
  it("parses ssh and https remotes", () => {
    expect(parseRepoFullNameFromRemote("git@github.com:org2ai/org2.git")).toBe(
      "org2ai/org2"
    );
    expect(
      parseRepoFullNameFromRemote("https://github.com/org2ai/org2.git")
    ).toBe("org2ai/org2");
    expect(parseRepoFullNameFromRemote("https://github.com/org2ai/org2")).toBe(
      "org2ai/org2"
    );
  });

  it("returns null for anything that is not a remote URL", () => {
    expect(parseRepoFullNameFromRemote("/local/path")).toBeNull();
  });
});

describe("resolveGithubRepoFullName", () => {
  it("picks the first github.com remote", () => {
    expect(
      resolveGithubRepoFullName([
        "git@gitlab.com:team/thing.git",
        "https://github.com/org2ai/org2.git",
      ])
    ).toBe("org2ai/org2");
  });

  it("ignores non-github hosts, including enterprise", () => {
    expect(
      resolveGithubRepoFullName(["git@github.enterprise.dev:team/thing.git"])
    ).toBeNull();
    expect(
      resolveGithubRepoFullName(["https://gitlab.com/a/b.git"])
    ).toBeNull();
  });

  it("rejects anything that is not exactly owner/repo", () => {
    // A blob URL cached as a remote must never be pasted into an API path.
    expect(
      resolveGithubRepoFullName(["https://github.com/org2ai/org2/tree/main"])
    ).toBeNull();
  });

  it("handles an absent or empty remote list", () => {
    expect(resolveGithubRepoFullName(undefined)).toBeNull();
    expect(resolveGithubRepoFullName([])).toBeNull();
  });
});
