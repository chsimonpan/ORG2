import { z } from "zod/v4";

import { defineProcedure } from "../invoke";
import * as schemas from "../schemas/globalPathExemptions";

export const globalPathExemptions = {
  list: defineProcedure("global_path_exemptions_list")
    .input(schemas.ListInput)
    .output(z.array(schemas.EntrySchema))
    .build(),
  add: defineProcedure("global_path_exemptions_add")
    .input(schemas.AddInput)
    .output(z.array(schemas.EntrySchema))
    .build(),
  remove: defineProcedure("global_path_exemptions_remove")
    .input(schemas.RemoveInput)
    .output(z.array(schemas.EntrySchema))
    .build(),
} as const;
