// The form that opens when the Prop tool marks a spot: the prop's look, state, optional readable
// text and (Advanced) channel. Apply sends one `create` with a fresh ref; the server stamps the
// owner. The form closes when that create is done, and shows the client's reason when it is refused.

import { Alert, Button, Group, Paper, Select, Stack, Text, TextInput } from "@mantine/core";
import { useEffect, useState } from "react";
import type { Command } from "../../game/editor";
import type { Submit } from "./follow";
import { createEntity, type Edits, newPropAt, type PropForm, type PropSpot, propOf, retryCommand, UNITS_PER_BLOCK } from "./state";

/** The codes a look or a state can take (0 to 7), as select options. */
const CODES = ["0", "1", "2", "3", "4", "5", "6", "7"].map((c) => ({ value: c, label: c }));

interface Props {
  spot: PropSpot;
  /** the prop's name in the catalogue, or the unknown-prop label */
  name: string;
  advanced: boolean;
  edits: Edits;
  submit: Submit;
  retry: (ref: number) => void;
  forget: (ref: number) => void;
  /** called when the create is done, or when the author lets go of the form */
  onClose: () => void;
}

export function PlaceProp({ spot, name, advanced, edits, submit, retry, forget, onClose }: Props) {
  const [form, setForm] = useState<PropForm>({ variant: "0", state: "0", text: "", channel: "" });
  const [pending, setPending] = useState<number>();
  const [problem, setProblem] = useState<string>();
  const mine = pending === undefined ? undefined : edits.get(pending);

  // the create is done when its edit leaves the open edits
  useEffect(() => {
    if (pending !== undefined && !edits.has(pending)) onClose();
  }, [edits, pending, onClose]);

  const apply = () => {
    const built = propOf(form, newPropAt(spot), advanced);
    if (!built.ok) {
      setProblem(built.error);
      return;
    }
    // a new try after a refusal is a new edit: let go of the old one first
    if (pending !== undefined) forget(pending);
    setProblem(undefined);
    const prop = built.value;
    setPending(submit((ref: number): Command => createEntity({ Prop: prop }, ref)));
  };

  const busy = mine?.status === "sent";
  const set = (patch: Partial<PropForm>) => setForm((f) => ({ ...f, ...patch }));

  return (
    <Paper withBorder shadow="sm" p="sm" role="dialog" aria-label="Place prop">
      <Stack gap="sm">
        <Group justify="space-between" wrap="nowrap">
          <Text fw={700} fz="sm">
            Place prop
          </Text>
          <Button size="xs" variant="subtle" onClick={onClose}>
            Close
          </Button>
        </Group>
        <Text fz="sm">{name}</Text>
        <Text fz="xs" c="moon.5">
          At {spot.at.map((v) => v / UNITS_PER_BLOCK).join(", ")} blocks, turned {spot.yaw * 15}°.
        </Text>
        <Group grow gap="xs">
          <Select
            label="Look"
            data={CODES}
            value={form.variant}
            allowDeselect={false}
            disabled={busy}
            onChange={(v) => v && set({ variant: v })}
          />
          <Select
            label="State"
            data={CODES}
            value={form.state}
            allowDeselect={false}
            disabled={busy}
            onChange={(v) => v && set({ state: v })}
          />
        </Group>
        <TextInput
          label="Text (optional)"
          description="What a readable says, at most 140 characters"
          value={form.text}
          disabled={busy}
          onChange={(e) => set({ text: e.currentTarget.value })}
        />
        {advanced && (
          <TextInput
            label="Channel (optional)"
            description="A number from 0 to 65535 that a switch or light listens to"
            inputMode="numeric"
            value={form.channel}
            disabled={busy}
            onChange={(e) => set({ channel: e.currentTarget.value })}
          />
        )}
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
          {busy ? "Sending…" : "Place prop"}
        </Button>
      </Stack>
    </Paper>
  );
}
