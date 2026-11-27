// The right column: the selected entity's data as plain text. Forms come in a later bead; for
// now the raw fields are shown so an author can see what the editor holds.

import { Code, Stack, Text } from "@mantine/core";
import type { Row } from "./state";
import { labelOf } from "./state";

export function Inspector({ row }: { row: Row | undefined }) {
  return (
    <Stack gap="xs" p="sm" w={{ base: "100%", md: 260 }} style={{ flexShrink: 0 }}>
      <Text fw={700} fz="sm">
        Inspector
      </Text>
      {row ? (
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
