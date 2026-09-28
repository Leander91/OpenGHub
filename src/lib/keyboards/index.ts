/**
 * The key map to draw for a keyboard: from its imported G HUB depot when
 * there is one (positions, and the render underneath), otherwise the drawn
 * fallback for the models OpenGHub knows.
 */
import { artworkIds } from "$lib/device-ui";
import { artwork } from "$lib/stores/artwork.svelte";
import type { Device } from "$lib/types";
import { keysFromLayout } from "./fromLayout";
import { G915_IDS, G915_KEYS, G915_SIZE, type Key } from "./g915";

export type { Key };

export interface KeyMap {
  keys: Key[];
  size: { w: number; h: number };
  /** The device render the keys sit on, when drawn from the depot. */
  image: string | null;
}

export function keyMapFor(device: Device): KeyMap | null {
  const ids = artworkIds(device);
  const front = artwork.layoutFor(ids)?.views.find((v) => v.view === "front");
  const fromDepot = front ? keysFromLayout(front) : null;
  if (fromDepot) return { ...fromDepot, image: artwork.forProductIds(ids) };
  if (ids.some((id) => G915_IDS.includes(id))) return { keys: G915_KEYS, size: G915_SIZE, image: null };
  return null;
}
