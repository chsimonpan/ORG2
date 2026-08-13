# CLI Version Scan Architecture Audit

## Acceptance criteria

- [x] A version scan starts only after the user selects a CLI in Session Creator.
- [x] Each actual scan probes the exact resolved executable with `--version` and queries its read-only official release source.
- [x] A fresh per-CLI observation is reused for twelve hours; there is no timer, scan-all path, or background polling loop.
- [x] Version scanning reads no credentials, invokes no updater, and writes nothing to Key Vault.
- [x] Session creation remains available when installed/latest version detection fails.
- [x] The inline warning appears only when both versions are comparable and the installed version is older.
- [x] Chat history preserves every CLI error, including repeated errors and errors preceding a recovered final reply.

## 10-layer review

| Layer                      | Scope checked                                                              | Verdict | Evidence                                                                                                          |
| -------------------------- | -------------------------------------------------------------------------- | ------- | ----------------------------------------------------------------------------------------------------------------- |
| 1. Compilation             | integrations resolver, key-vault command, Tauri handler, RPC/Zod, React    | pass    | `cargo check -p org2 --bin org2` and `pnpm typecheck`                                                             |
| 2. Dead code / duplication | version parsing, status comparison, old repeated-error UI                  | pass    | installed parsing is resolver-owned; latest parsing/status is command-owned; repeat-count field/rendering removed |
| 3. Naming                  | scan vs update; installed/latest/current/outdated/unknown                  | pass    | public names describe observations and never imply mutation                                                       |
| 4. Semantic overloading    | unavailable source, network failure, unparseable version, older version    | pass    | only a comparable older version maps to `outdated`; all other indeterminate cases map to `unknown`                |
| 5. Default branches        | missing binary, missing release source, timeout, endpoint error, cache hit | pass    | bounded diagnostics are returned without blocking session creation                                                |
| 6. Cross-domain leakage    | Key Vault and credential environment                                       | pass    | scanner receives an agent type and resolved binary only; snapshot contains version metadata only                  |
| 7. New-developer confusion | cache ownership and scan trigger                                           | pass    | comments identify the twelve-hour backend/frontend caches and selection-only trigger                              |
| 8. Wire protocol           | Rust snapshot ↔ Zod ↔ typed service/hook                                   | pass    | field names and enum values match exactly across the boundary                                                     |
| 9. Init parity             | first selection, re-render, concurrent calls, forced future refresh        | pass    | frontend deduplicates promises; backend is authoritative for the twelve-hour cache                                |
| 10. Resolver symmetry      | discovered binary vs launched binary                                       | pass    | version probe uses the centralized resolved executable path, not a new PATH lookup                                |

## Default-branch analysis

| State                                  | Result                                | Session impact                                     |
| -------------------------------------- | ------------------------------------- | -------------------------------------------------- |
| installed < latest                     | `outdated`                            | warning shown; launch remains enabled              |
| installed = latest                     | `current`                             | no warning                                         |
| installed > latest                     | `current`                             | no false warning for prerelease/newer local builds |
| installed/latest missing or unparsable | `unknown`                             | no warning; CLI errors remain visible in Chat      |
| release endpoint fails/times out       | `unknown` with bounded diagnostic     | no warning; no retry loop                          |
| fresh cached snapshot                  | returned without process/network work | no added CPU/network churn                         |

## Findings

No unresolved architecture finding remains in this scope. Three CLIs currently have no stable read-only latest-version endpoint; they still report the installed version and remain `unknown` rather than invoking a self-updater or fabricating a latest version. Kiro and Antigravity use the version field from their official read-only release manifests.
