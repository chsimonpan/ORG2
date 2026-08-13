# ChatHistory architecture audit

Scope: the `ChatHistory` public component, its extracted projection, navigation, viewport, item-action, planning-indicator, and render-view modules, plus the existing state/search/pagination hooks they compose.

## Acceptance criteria

- `ChatHistory/index.tsx` is a small composition boundary rather than a state, derivation, effect, and rendering monolith.
- Public props and default behavior remain compatible.
- Raw history projection, visible turn pagination, navigation, viewport effects, item actions, and JSX each have one clear owner.
- The extracted hooks are used by the production entry point and do not create circular imports.
- Tail-turn collapse and conversation-page mapping have direct characterization tests.
- TypeScript, focused lint, the complete ChatHistory test surface, and the circular-dependency check pass; unrelated full-suite failures are identified explicitly.

## Term overloading sweep

| Term             | Meaning in this change                                                                | Verdict                                                                                       |
| ---------------- | ------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| history          | The raw event list supplied to `ChatHistory`                                          | keep with reason — the existing public prop remains stable                                    |
| projected groups | Grouped render records after filtering and display projection                         | pass — explicitly owned by `useChatHistoryProjectionModel`                                    |
| turn page        | The visible slice of projected groups selected by turn pagination                     | pass — kept distinct from browser/history-page navigation                                     |
| active group     | The clamped group used for minimap and overview selection                             | pass — derived in the navigation controller instead of mirrored by a repair effect            |
| item actions     | Edit, checkpoint restore, regenerate, and answer callbacks for rendered history items | pass — `useChatHistoryItemActions` avoids collision with the existing history-actions context |

## Ten-layer audit

| Layer | Area inspected                       | Verdict        | Reason / evidence                                                                                                                                                                                                                                                                             | Suggested change                                                                   |
| ----: | ------------------------------------ | -------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
|     1 | Compilation correctness              | pass in scope  | TypeScript and focused ESLint pass for every extracted module. The 21-file ChatHistory suite passes all 294 tests. The repository-wide run reaches 469 passing files but is blocked by a modified websocket schema test outside this scope and two dependency-level `ERR_REQUIRE_ESM` errors. | Resolve the websocket/schema and test-environment failures in their owning change. |
|     2 | Dead code and structural duplication | pass           | `index.tsx` imports and invokes every extracted production hook; the render view consumes their returned models. No parallel copy of the former orchestration remains.                                                                                                                        | None.                                                                              |
|     3 | Naming consistency                   | pass after fix | The initially ambiguous action-hook name was changed to `useChatHistoryItemActions`, separating item callbacks from the existing `useChatHistoryActions` context hook.                                                                                                                        | None.                                                                              |
|     4 | Semantic overloading                 | pass           | Raw history, projected groups, visible turn pages, and external history-page navigation are represented by separate names and owners.                                                                                                                                                         | None.                                                                              |
|     5 | Default branches                     | pass           | Existing defaults remain explicit in `ChatHistory.types.ts` and the entry point: standard grouping, collapsed groups, and inert external-navigation contracts.                                                                                                                                | None.                                                                              |
|     6 | Cross-domain leakage                 | pass           | Planning atom reads are isolated in `PlanningIndicatorBridge`; the view receives render data and callbacks without reaching into projection or viewport state. Agent-org data remains an explicit view prop.                                                                                  | None.                                                                              |
|     7 | New-developer clarity                | pass           | The 206-line entry point reads top-to-bottom as state, projection, navigation, viewport, actions, and view composition. Each extracted filename describes its responsibility.                                                                                                                 | None.                                                                              |
|     8 | Wire protocol / serialization        | not applicable | No network payload, persistence shape, schema, or serialization boundary changed.                                                                                                                                                                                                             | None.                                                                              |
|     9 | Entry-point parity                   | not applicable | `ChatHistory` has one public production entry point; this refactor did not add alternate initialization paths.                                                                                                                                                                                | None.                                                                              |
|    10 | Resolver symmetry                    | not applicable | No multi-source configuration or identity resolver changed.                                                                                                                                                                                                                                   | None.                                                                              |

## Systematic sweep

- Traced the public `ChatHistory` entry point through each new hook and into `ChatHistoryView`; every new production abstraction is on that call chain.
- Searched the hook barrel and direct imports for the action-hook naming class; the item-action hook is now distinct from the existing context action hook.
- Ran the repository circular-dependency check across 5,286 files; it reported no circular dependency.
- Kept public prop exports at the package entry point while moving their definitions to `ChatHistory.types.ts`, avoiding duplicate prop contracts.
- Ran the repository-wide test suite: 469 files / 5,247 tests passed; `src/api/realtime/websocket/schemas.test.ts` has one out-of-scope failure in an independently modified file, and the runner reports two `html-encoding-sniffer` / `@exodus/bytes` module-format errors.

## Summary

- **fixes applied: 1** — resolved the action-hook naming collision.
- **abstractions introduced: 7** — projection, navigation, viewport, item actions, tail collapse, planning bridge, and render-only view.
- **kept with reason: 3** — public prop names/defaults, the existing hook barrel, and view-local `useGroupHeaderRenderer` composition.
- **not applicable layers: 3** — wire protocol, entry-point parity, and resolver symmetry.
