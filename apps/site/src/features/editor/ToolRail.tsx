// The left column: the tools (each sends a tool command to the client), then the block palette
// while the Blocks tool is held, or the prop catalogue while the Prop tool is held.

import { Box, Button, Stack, Text, Tooltip } from "@mantine/core";
import type { Tool } from "../../game/editor";
import { BlockPalette } from "./BlockPalette";
import { type Catalogue, PropCatalogue } from "./PropCatalogue";

interface Props {
  active: Tool;
  onTool: (t: Tool) => void;
  advanced: boolean;
  /** the world's block names, from `ready` */
  palette: string[];
  /** the block the page has chosen to paint with */
  block?: string;
  onBlock: (name: string) => void;
  /** the prop the page has chosen to place, by hash */
  prop?: string;
  onProp: (hash: string) => void;
  /** the catalogue the page loaded */
  catalogue: Catalogue;
}

const TOOLS: { tool: Tool; label: string; soon?: boolean; advancedOnly?: boolean }[] = [
  { tool: "select", label: "Select" },
  { tool: "blocks", label: "Blocks" },
  { tool: "portal", label: "Portal" },
  { tool: "prop", label: "Prop" },
  { tool: "node", label: "Node", soon: true, advancedOnly: true },
];

export function ToolRail({ active, onTool, advanced, palette, block, onBlock, prop, onProp, catalogue }: Props) {
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
      {active === "blocks" ? (
        <BlockPalette palette={palette} chosen={block} onChoose={onBlock} />
      ) : active === "prop" ? (
        <PropCatalogue entries={catalogue.entries} error={catalogue.error} chosen={prop} onChoose={onProp} />
      ) : (
        <>
          <Text fw={700} fz="sm" mt="md">
            Palette
          </Text>
          <Text c="moon.5" fz="sm">
            Blocks and props appear here while those tools are held.
          </Text>
        </>
      )}
    </Stack>
  );
}
