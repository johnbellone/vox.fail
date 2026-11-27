// The bottom strip: every entity the editor knows about. Clicking one selects it and flies the
// camera to it.

import { Button, Group, Stack, Text } from "@mantine/core";
import { type Command, sendCommand } from "../../game/editor";
import { labelOf, type Row } from "./state";

interface Props {
  rows: Row[];
  selected: number | null;
  onError: (text: string) => void;
}

export function EntityList({ rows, selected, onError }: Props) {
  const go = (entity: number) => {
    const commands: Command[] = [
      { type: "select", entity },
      { type: "fly_to", entity },
    ];
    try {
      commands.forEach(sendCommand);
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    }
  };
  return (
    <Stack gap="xs" p="sm">
      <Text fw={700} fz="sm">
        Entities <Text component="span" c="moon.5" fz="sm" fw={400}>(click to fly there)</Text>
      </Text>
      {rows.length === 0 ? (
        <Text c="moon.5" fz="sm">
          Nothing here yet.
        </Text>
      ) : (
        <Group gap="xs" wrap="wrap">
          {rows.map((r) => (
            <Button
              key={r.entity}
              size="xs"
              variant={r.entity === selected ? "filled" : "default"}
              onClick={() => go(r.entity)}
            >
              {labelOf(r)}
            </Button>
          ))}
        </Group>
      )}
    </Stack>
  );
}
