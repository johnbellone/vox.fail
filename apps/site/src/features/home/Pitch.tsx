// What Vox is, in a few lines: the left half of the home page, and the offline page's (apps/edge).

import { Stack, Text } from "@mantine/core";
import { Wordmark } from "../../components/Wordmark";

export function Pitch() {
  return (
    <Stack gap="md">
      <Wordmark />
      <Text fz="xl" fw={700} c="moon.9">
        A world built from blocks, shared with everyone in it. What you build stays.
      </Text>
      <Text c="moon.5">
        Everyone arrives in the Void: a bright floor that goes on for ever, a circle of pale pillars, and doorways
        that open onto other places as you walk up to them.
      </Text>
      <Text c="moon.5">
        Time spent building earns authority, the standing every world recognises. Spend it on feats: claim a plot
        that only you and your group can build in, open a portal to another world, give a world you own its own coin,
        or lead your friends on a raid.
      </Text>
    </Stack>
  );
}
