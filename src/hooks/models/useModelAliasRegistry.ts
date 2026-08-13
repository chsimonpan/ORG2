/**
 * Populates the model alias registry from key records so model icons and labels
 * can resolve user-chosen aliases globally.
 *
 * Mount once at app level.
 */
import { useEffect } from "react";

import { loadSharedLocalKeys } from "@src/hooks/keyVault/sharedLocalKeyStore";

export function useModelAliasRegistry(): void {
  useEffect(() => {
    let cancelled = false;

    async function populate() {
      await loadSharedLocalKeys();
      if (cancelled) return;
    }

    void populate();
    return () => {
      cancelled = true;
    };
  }, []);
}
