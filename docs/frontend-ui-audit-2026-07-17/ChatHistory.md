# Frontend UI audit — ChatHistory

**Primary file:** `src/engines/ChatPanel/ChatHistory/components/ChatHistoryView.tsx` (523 LOC)

**Composition file:** `src/engines/ChatPanel/ChatHistory/index.tsx` (206 LOC)

**Date:** 2026-07-17

> **Method note:** the `frontend-ui-audit` skill referenced by `AGENTS.md` was unavailable at both documented locations. This is a manual fallback review using the repository's stated dimensions: design-system usage, arbitrary Tailwind values, hardcoded presentation values, accessibility, and repeated visual patterns. It is a post-refactor review of an implementation that intentionally preserves the existing DOM and class names.

## D1 — Raw HTML vs design system

| Line                          | Element                       | Verdict          | Reason                                                                                                                                | Suggested change |
| ----------------------------- | ----------------------------- | ---------------- | ------------------------------------------------------------------------------------------------------------------------------------- | ---------------- |
| `ChatHistoryView.tsx:318`     | History content-width wrapper | keep with reason | It uses the shared `DETAIL_PANEL_TOKENS.contentWidth`; the wrapper is structural and has no matching interactive design-system role.  | None.            |
| `ChatHistoryView.tsx:352`     | Overview panel                | keep with reason | It consumes `DROPDOWN_CLASSES.panel`, preserving the shared panel surface rather than duplicating its border/background treatment.    | None.            |
| `ChatHistoryView.tsx:418-422` | Loading state                 | keep with reason | The existing `Spinner` component and `SPINNER_TOKENS.default` remain the canonical loading treatment.                                 | None.            |
| `ChatHistoryView.tsx:213`     | Group-header renderer         | abstract         | Header UI remains centralized in `useGroupHeaderRenderer`; the view composes it instead of recreating variant-specific header markup. | None.            |

## D2 — Arbitrary Tailwind values vs tokens

| Line                              | Element                    | Verdict          | Reason                                                                                                                                                                        | Suggested change |
| --------------------------------- | -------------------------- | ---------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------- |
| `ChatHistoryView.tsx:345`         | `max-h-[45%]` overview cap | keep with reason | This is the existing context-specific viewport proportion, moved without change. It has no semantically equivalent shared token and does not represent a repeated size scale. | None.            |
| `ChatHistoryView.tsx:318,348,416` | Detail-panel width classes | keep with reason | Width behavior comes from `DETAIL_PANEL_TOKENS` rather than new arbitrary values.                                                                                             | None.            |

## D3 — Hardcoded sizes and colors

| Line                          | Value                                        | Verdict          | Reason                                                                                                                                              | Suggested change |
| ----------------------------- | -------------------------------------------- | ---------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------- |
| `ChatHistoryView.tsx:248-250` | Dynamic chat font CSS variables              | keep with reason | These values are user-configured runtime settings and therefore cannot be represented by static Tailwind tokens.                                    | None.            |
| `ChatHistoryView.tsx:44`      | One-pixel hidden header placeholder          | keep with reason | The minimum height preserves virtualized layout behavior when group headers are disabled; it is a functional measurement, not a visual-system size. | None.            |
| `ChatHistoryView.tsx:318-419` | Surface, width, dropdown, and spinner tokens | keep with reason | The extraction retains existing semantic tokens and adds no hardcoded color.                                                                        | None.            |

## D4 — Accessibility

| Line                      | Element                                | Verdict          | Reason                                                                                                                             | Suggested change |
| ------------------------- | -------------------------------------- | ---------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------- |
| `ChatHistoryView.tsx:44`  | Empty group-header placeholder         | keep with reason | `aria-hidden` prevents the layout-only element from entering the accessibility tree.                                               | None.            |
| `ChatHistoryView.tsx:309` | Message list test/interaction boundary | keep with reason | The extraction preserves the existing DOM identity and does not replace native interactive descendants with generic click targets. | None.            |
| Entire view               | Focus and keyboard behavior            | keep with reason | JSX was moved, not redesigned; existing component boundaries, handlers, aria attributes, and rendered order remain intact.         | None.            |

## D5 — Repeated visual patterns

| Pattern                                           | Sites              | Count | Verdict          | Suggested change                                                                                         |
| ------------------------------------------------- | ------------------ | ----: | ---------------- | -------------------------------------------------------------------------------------------------------- |
| Chat history render tree mixed with orchestration | Former `index.tsx` |     1 | abstract         | Extracted once into `ChatHistoryView`; no generic repository-wide component is warranted.                |
| Detail-panel content widths                       | View wrappers      |     3 | keep with reason | Already centralized by `DETAIL_PANEL_TOKENS`; no additional wrapper abstraction would improve ownership. |
| Dropdown-style overview surface                   | Overview panel     |     1 | keep with reason | Already consumes the shared dropdown surface token.                                                      |

## Summary

- **fix candidates: 0** — the refactor adds no new visual-system or accessibility debt.
- **kept with documented reason: 10** — structural HTML, one functional arbitrary percentage, runtime CSS variables, accessibility-preserving placeholders, and existing tokens.
- **abstracted: 2** — the render tree moved to `ChatHistoryView`, while group-header rendering remains centralized in its existing renderer hook.
- **systematic sweep candidates: 0** — no newly introduced pattern spans multiple source files.
