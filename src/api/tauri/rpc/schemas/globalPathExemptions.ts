import { z } from "zod/v4";

export const PathSchema = z
  .string()
  .min(1)
  .refine((path) => !path.includes("\0"), "NUL path denied");
export const EntrySchema = z
  .object({
    path: PathSchema,
    mode: z.enum(["read", "read_write"]),
    recursive: z.boolean(),
  })
  .strict();
export const ListInput = z.object({}).strict();
export const AddInput = EntrySchema;
export const RemoveInput = z.object({ path: PathSchema }).strict();
