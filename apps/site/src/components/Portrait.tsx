import { Avatar } from "@mantine/core";
import type { Catalogue } from "../api/types";
import { findCast } from "../api/client";

/** A character's head-and-shoulders picture, by model. */
export function Portrait({ cat, model, size = 40 }: { cat: Catalogue | undefined; model: string | undefined; size?: number }) {
  const c = findCast(cat, model);
  return <Avatar src={`/assets/${c?.portrait ?? "portraits/blue.png"}`} alt="" size={size} radius="md" bg="moon.1" />;
}
