// The form that adds a graph node (Advanced only), opened by the Add node button: its kind, where
// its footprint starts (typed, or the marked spot) and its size. Apply sends one `create` with a fresh
// ref; the node is not refined, has no recipe, and gets a random seed. The form closes when that
// create is done, and shows the client's reason when it is refused.

import { Alert, Button, Group, Paper, Stack, Text, TextInput } from "@mantine/core";
import { useEffect, useState } from "react";
import type { Command } from "../../game/editor";
import type { Submit } from "./follow";
import { TripleFields } from "./SpawnObjective";
import {
  createEntity,
  type Edits,
  newNodeFormOf,
  type NodeForm,
  nodeOf,
  retryCommand,
  type Spot,
  textOf,
} from "./state";

interface Props {
  /** the spot the Portal tool last marked, where the footprint may start */
  spot?: Spot;
  edits: Edits;
  submit: Submit;
  retry: (ref: number) => void;
  forget: (ref: number) => void;
  /** called when the create is done, or when the author lets go of the form */
  onClose: () => void;
}

export function NewNode({ spot, edits, submit, retry, forget, onClose }: Props) {
  const [form, setForm] = useState<NodeForm>(() => newNodeFormOf(spot));
  const [pending, setPending] = useState<number>();
  const [problem, setProblem] = useState<string>();
  const mine = pending === undefined ? undefined : edits.get(pending);

  // the create is done when its edit leaves the open edits
  useEffect(() => {
    if (pending !== undefined && !edits.has(pending)) onClose();
  }, [edits, pending, onClose]);

  const apply = () => {
    const built = nodeOf(form, null);
    if (!built.ok) {
      setProblem(built.error);
      return;
    }
    // a new try after a refusal is a new edit: let go of the old one first
    if (pending !== undefined) forget(pending);
    setProblem(undefined);
    const node = built.value;
    setPending(submit((ref: number): Command => createEntity({ Node: node }, ref)));
  };

  const busy = mine?.status === "sent";
  const set = (patch: Partial<NodeForm>) => setForm((f) => ({ ...f, ...patch }));

  return (
    <Paper withBorder shadow="sm" p="sm" role="dialog" aria-label="Add node">
      <Stack gap="sm">
        <Group justify="space-between" wrap="nowrap">
          <Text fw={700} fz="sm">
            Add node
          </Text>
          <Button size="xs" variant="subtle" onClick={onClose}>
            Close
          </Button>
        </Group>
        <TextInput
          label="Kind"
          description="A name of lower-case letters, digits and underscores, such as house"
          value={form.kind}
          disabled={busy}
          onChange={(e) => set({ kind: e.currentTarget.value })}
        />
        <Stack gap={4}>
          <TripleFields
            label="Position (blocks)"
            names={["x", "y", "z"]}
            values={form.at}
            disabled={busy}
            onChange={(i, v) => {
              const at = [...form.at] as [string, string, string];
              at[i] = v;
              set({ at });
            }}
          />
          {spot && (
            <Button
              size="xs"
              variant="default"
              disabled={busy}
              onClick={() => set({ at: textOf(spot.at.map((v) => Math.floor(v)) as [number, number, number]) })}
              w="fit-content"
            >
              Use marked spot
            </Button>
          )}
        </Stack>
        <TripleFields
          label="Size (blocks, 1 to 64 each)"
          names={["x", "y", "z"]}
          values={form.size}
          disabled={busy}
          onChange={(i, v) => {
            const size = [...form.size] as [string, string, string];
            size[i] = v;
            set({ size });
          }}
        />
        {problem && (
          <Alert color="ember" role="alert">
            {problem}
          </Alert>
        )}
        {mine?.status === "refused" && pending !== undefined && (
          <Alert color="ember" role="alert">
            <Stack gap="xs">
              <Text fz="sm">{mine.text}</Text>
              {retryCommand(edits, pending) !== undefined && (
                <Button size="xs" variant="default" onClick={() => retry(pending)} w="fit-content">
                  Try again
                </Button>
              )}
            </Stack>
          </Alert>
        )}
        <Button size="sm" onClick={apply} disabled={busy} w="fit-content">
          {busy ? "Sending…" : "Add node"}
        </Button>
      </Stack>
    </Paper>
  );
}
