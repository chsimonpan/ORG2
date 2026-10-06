import type { ModelGroup } from "./modelGrouping";

/** Presentation-only filter: it never produces an enabled_models payload. */
export function filterAccountModelGroups(
  groups: readonly ModelGroup[],
  query: string,
  provider = ""
): ModelGroup[] {
  const needle = query.trim().toLocaleLowerCase();
  if (!needle) return [...groups];
  return groups.flatMap((group) => {
    const commonMatch = [group.label, provider].some((text) =>
      text.toLocaleLowerCase().includes(needle)
    );
    const models = commonMatch
      ? group.models
      : group.models.filter((id) => id.toLocaleLowerCase().includes(needle));
    return models.length ? [{ ...group, models }] : [];
  });
}
