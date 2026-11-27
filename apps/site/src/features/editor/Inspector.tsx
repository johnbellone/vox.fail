// The right column: a selected portal or prop gets its form, and a node its form in Advanced; any
// other entity shows its raw data.

import { Code, Stack, Text } from "@mantine/core";
import type { Item, WorldEntry } from "../../api/types";
import type { Command } from "../../game/editor";
import { NodeInspector } from "./NodeInspector";
import { PortalInspector } from "./PortalInspector";
import { PropInspector } from "./PropInspector";
import { type Edits, labelOf, type PropEntry, propNameOf, type Row } from "./state";

interface Props {
  row: Row | undefined;
  /** the catalogue's props, for a prop's name */
  catalogue: readonly PropEntry[];
  advanced: boolean;
  worlds: readonly WorldEntry[];
  items: readonly Item[];
  itemsError?: string;
  edits: Edits;
  submit: (make: (ref: number) => Command) => number;
  retry: (ref: number) => void;
  forget: (ref: number) => void;
}

export function Inspector({ row, catalogue, advanced, worlds, items, itemsError, edits, submit, retry, forget }: Props) {
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
      ) : row && "Prop" in row.component ? (
        <PropInspector
          key={row.entity}
          entity={row.entity}
          seq={row.seq}
          prop={row.component.Prop}
          name={propNameOf(row.component.Prop.prop, catalogue)}
          advanced={advanced}
          edits={edits}
          submit={submit}
          retry={retry}
          forget={forget}
        />
      ) : row && "Node" in row.component && advanced ? (
        <NodeInspector
          key={row.entity}
          entity={row.entity}
          seq={row.seq}
          node={row.component.Node}
          edits={edits}
          submit={submit}
          retry={retry}
          forget={forget}
        />
      ) : row ? (
        <>
          <Text fz="sm">{labelOf(row, catalogue)}</Text>
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
