// The block palette, shown in the left column while the Blocks tool is held: every block of the
// world (the names the client sent with `ready`), in a scrollable grid. Choosing one tells the
// client to paint with it; the choice is the page's, the client checks the name against the
// world's palette again.

import { Box, Button, SimpleGrid, Text } from "@mantine/core";
import type { Command } from "../../game/editor";

/** The command that makes `name` the block to paint with. */
export function chooseBlock(name: string): Command {
  return { type: "block", name };
}

interface Props {
  palette: string[];
  chosen?: string;
  onChoose: (name: string) => void;
}

export function BlockPalette({ palette, chosen, onChoose }: Props) {
  return (
    <Box>
      <Text fw={700} fz="sm" mb="xs">
        Blocks
      </Text>
      {palette.length === 0 ? (
        <Text c="moon.5" fz="sm">
          The world's blocks appear once it is open.
        </Text>
      ) : (
        <Box mah={260} style={{ overflowY: "auto" }} aria-label="Block palette">
          <SimpleGrid cols={2} spacing={4}>
            {palette.map((name) => (
              <Button
                key={name}
                size="compact-xs"
                variant={name === chosen ? "filled" : "default"}
                aria-pressed={name === chosen}
                title={name}
                onClick={() => onChoose(name)}
                styles={{ label: { overflow: "hidden", textOverflow: "ellipsis" } }}
              >
                {name}
              </Button>
            ))}
          </SimpleGrid>
        </Box>
      )}
    </Box>
  );
}
