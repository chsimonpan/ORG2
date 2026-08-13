# CLI Version Alert Frontend UI Audit

The repository-routed frontend UI audit skill was unavailable, so this report applies the repository's required columns manually.

| Line / element                                | Element                      | Verdict          | Reason                                                                                                                    | Suggested change                                                  |
| --------------------------------------------- | ---------------------------- | ---------------- | ------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------- |
| `SessionCreator/variants/ChatPanel/index.tsx` | selected-CLI scan trigger    | keep with reason | Session Creator's selected CLI atom is the authoritative demand signal; the shared twelve-hour cache prevents repeat work | Keep selection-only and never add a mount-time scan-all effect    |
| `SessionCreator/variants/ChatPanel/index.tsx` | version warning              | keep with reason | Existing `InlineAlert` matches the warning pattern already used below the creator and does not block launch               | Show installed/latest values and leave upgrade action to the user |
| `ExtendedItemRenderers.tsx`                   | repeated error notice        | fix              | A count-only summary hides the individual provider error payloads the user needs for diagnosis                            | Render every error item independently                             |
| `useChatGroups.ts`                            | collapsed-turn pinned errors | fix              | Errors before a later successful reply are still useful diagnostics                                                       | Pin all agent errors regardless of their position in the turn     |

## Summary

- Fix: 2
- Keep with reason: 2
- Abstract: 0
