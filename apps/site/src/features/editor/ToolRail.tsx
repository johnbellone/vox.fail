// The left column: the tools (each sends a tool command to the client) and a placeholder for
// the block palette and catalogue, which later beads fill in.

import { Box, Button, Stack, Text, Tooltip } from "@mantine/core";
import type { Tool } from "../../game/editor";

interface Props {
  active: Tool;
  onTool: (t: Tool) => void;
  advanced: boolean;
}

const TOOLS: { tool: Tool; label: string; soon?: boolean; advancedOnly?: boolean }[] = [
  { tool: "select", label: "Select" },
  { tool: "blocks", label: "Blocks" },
  { tool: "portal", label: "Portal" },
  { tool: "prop", label: "Prop", soon: true },
  { tool: "node", label: "Node", soon: true, advancedOnly: true },
];

export function ToolRail({ active, onTool, advanced }: Props) {
  return (
    <Stack gap="xs" p="sm" w={{ base: "100%", md: 180 }} style={{ flexShrink: 0 }}>
      <Text fw={700} fz="sm">
        Tools
      </Text>
      {TOOLS.filter((t) => !t.advancedOnly || advanced).map((t) => {
        const button = (
          <Button
            variant={active === t.tool ? "filled" : "default"}
            size="sm"
            justify="flex-start"
            disabled={t.soon}
            aria-pressed={active === t.tool}
            onClick={() => onTool(t.tool)}
          >
            {t.label}
          </Button>
        );
        // a disabled button takes no pointer events, so the tooltip wraps a box around it
        return t.soon ? (
          <Tooltip key={t.tool} label="coming soon" withArrow>
            <Box>{button}</Box>
          </Tooltip>
        ) : (
          <Box key={t.tool}>{button}</Box>
        );
      })}
      <Text fw={700} fz="sm" mt="md">
        Palette
      </Text>
      <Text c="moon.5" fz="sm">
        Blocks and the prop catalogue appear here.
      </Text>
    </Stack>
  );
}
