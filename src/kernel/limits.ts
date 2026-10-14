// Lane 16b: the model's count limits on the app's side (crates/kernel/src/limits.rs keeps
// the kernel's, which refuses every edit past them; limits.test.ts reads that file and
// docs/LIMITS.md, so the three cannot drift apart). The app checks the same numbers where
// it can refuse earlier than the kernel: a .skp before it is mapped to an import (src/io/
// skp.ts), a plugin batch before it is sent (src/plugins/manager.ts).
import type { DocState, SceneData } from "./types";

export const MODEL_LIMITS = {
  guides: 50_000,
  dimensions: 50_000,
  tags: 10_000,
  materials: 4_096,
  /** Of the materials, the most with a texture image of the model's own (looks::MAX_TEXTURES). */
  textures: 512,
  scenes: 1_000,
  sections: 1_000,
  sheets: 200,
  sheetItems: 2_000,
} as const;

/** What a count is called in a refusal (the kernel's words). */
const WORDS: Record<keyof typeof MODEL_LIMITS, string> = {
  guides: "guides",
  dimensions: "dimensions",
  tags: "tags",
  materials: "materials",
  textures: "texture images",
  scenes: "scenes",
  sections: "section planes",
  sheets: "sheets",
  sheetItems: "items on one sheet",
};

export type ModelCounts = Partial<Record<keyof typeof MODEL_LIMITS, number>>;

/** A count past its limit (the kernel's LimitError::Count, said the same way). */
export class ModelLimitError extends Error {
  constructor(
    readonly what: keyof typeof MODEL_LIMITS,
    readonly count: number,
  ) {
    super(`the model would hold ${count} ${WORDS[what]}; at most ${MODEL_LIMITS[what]}`);
    this.name = "ModelLimitError";
  }
}

/** Throw a ModelLimitError for the first count past its limit. */
export function checkCounts(c: ModelCounts): void {
  for (const k of Object.keys(MODEL_LIMITS) as (keyof typeof MODEL_LIMITS)[]) {
    const n = c[k];
    if (n !== undefined && n > MODEL_LIMITS[k]) throw new ModelLimitError(k, n);
  }
}

/** What the document holds now, counted the way the limits are stated. */
export function countsOf(state: DocState | null | undefined, scene?: SceneData | null): Required<ModelCounts> {
  return {
    guides: state?.guides?.length ?? 0,
    dimensions: state?.dimensions?.length ?? 0,
    tags: Object.keys(state?.tags ?? {}).length,
    materials: state?.materials?.length ?? 0,
    textures: state?.textures?.length ?? 0,
    scenes: state?.scenes?.length ?? 0,
    sections: state?.graph?.sections?.length ?? 0,
    sheets: scene?.sheets?.length ?? 0,
    sheetItems: 0,
  };
}

/** True for a kernel refusal that is a limit (said "...; at most N" by crates/kernel/src/limits.rs). */
export function isLimitRefusal(message: string): boolean {
  return /(would hold|holds more than) [^;]*; (a model holds )?at most \d+/.test(message) || /is more than the \d+ one import may bring in/.test(message);
}
