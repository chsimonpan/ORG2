import { invoke } from "@tauri-apps/api/core";
import { z } from "zod/v4";

import {
  AddInput,
  EntrySchema,
  RemoveInput,
} from "@src/api/tauri/rpc/schemas/globalPathExemptions";

export type GlobalPathExemption = z.infer<typeof EntrySchema>;
const Entries = z.array(EntrySchema);
export const listGlobalPathExemptions = async () =>
  Entries.parse(await invoke<unknown>("global_path_exemptions_list"));
export const addGlobalPathExemption = async (grant: GlobalPathExemption) =>
  Entries.parse(
    await invoke<unknown>("global_path_exemptions_add", AddInput.parse(grant))
  );
export const removeGlobalPathExemption = async (path: string) =>
  Entries.parse(
    await invoke<unknown>(
      "global_path_exemptions_remove",
      RemoveInput.parse({ path })
    )
  );
