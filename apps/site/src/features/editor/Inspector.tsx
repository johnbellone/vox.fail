// The right column: a selected portal gets its form; any other entity shows its raw data for
// now (its forms come in a later bead).

import { Code, Stack, Text } from "@mantine/core";
import type { Item, WorldEntry } from "../../api/types";
import type { Command } from "../../game/editor";
import { PortalInspector } from "./PortalInspector";
import { type Edits, labelOf, type Row } from "./state";

interface Props {
  row: Row | undefined;
  advanced: boolean;
  worlds: readonly WorldEntry[];
  items: readonly Item[];
  itemsError?: string;
  edits: Edits;
  submit: (make: (ref: number) => Command) => number;
  retry: (ref: number) => void;
  forget: (ref: number) => void;
}

export function Inspector({ row, advanced, worlds, items, itemsError, edits, submit, retry, forget }: Props) {
  return (
    <Stack gap="xs" p="sm" w={{ base: "100%", md: 280 }} style={{ flexShrink: 0 }}>
      <Text fw={700} fz="sm">
        Inspector
      </Text>
      {row && "Portal" in row.component ? (
        <PortalInspector
          key={row.entity}
          entity={row.entity}
          seq={row.seq}
          portal={row.component.Portal}
          advanced={advanced}
          worlds={worlds}
          items={items}
          itemsError={itemsError}
          edits={edits}
          submit={submit}
          retry={retry}
          forget={forget}
        />
      ) : row ? (
        <>
          <Text fz="sm">{labelOf(row)}</Text>
          <Code block fz="xs">
            {JSON.stringify(row.component, null, 2)}
          </Code>
        </>
      ) : (
        <Text c="moon.5" fz="sm">
          Select something in the world or in the list below.
        </Text>
      )}
    </Stack>
  );
}
