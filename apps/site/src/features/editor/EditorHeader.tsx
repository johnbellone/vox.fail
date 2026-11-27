// The top bar: the world's name, the Basic/Advanced switch (only for those who may use it),
// Undo and Redo (not wired yet), and whether the page is live with others here.

import { Badge, Button, Group, SegmentedControl, Text } from "@mantine/core";

export type Mode = "basic" | "advanced";

interface Props {
  world: string;
  mode: Mode;
  onMode: (m: Mode) => void;
  /** whether this account may use Advanced controls in this world */
  canAdvanced: boolean;
  here: number;
}

export function EditorHeader({ world, mode, onMode, canAdvanced, here }: Props) {
  return (
    <Group justify="space-between" wrap="wrap" px="md" py="sm" gap="sm">
      <Group gap="sm" wrap="wrap">
        <Text fw={700} fz="lg">
          {world}
        </Text>
        {canAdvanced ? (
          <SegmentedControl
            size="xs"
            value={mode}
            onChange={(v) => onMode(v as Mode)}
            data={[
              { label: "Basic", value: "basic" },
              { label: "Advanced", value: "advanced" },
            ]}
          />
        ) : (
          <Text c="moon.5" fz="sm">
            Basic
          </Text>
        )}
      </Group>
      <Group gap="xs">
        <Button size="xs" variant="default" disabled>
          Undo
        </Button>
        <Button size="xs" variant="default" disabled>
          Redo
        </Button>
        <Badge color="gate" variant="light" aria-live="polite">
          ● live · {here} here
        </Badge>
      </Group>
    </Group>
  );
}
