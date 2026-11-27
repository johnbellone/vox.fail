// The form that opens when the Portal tool marks a spot: a name, a destination and the size
// (defaulted from the spot). Apply sends one `create` with a fresh ref and a null key (the
// directory gives a player's portal its key; an editor's portal has none). The form closes when
// that edit is done, and shows the client's reason when it is refused.

import { Alert, Button, Group, Paper, Stack, Text } from "@mantine/core";
import { useEffect, useState } from "react";
import type { Item, WorldEntry } from "../../api/types";
import type { Command } from "../../game/editor";
import { PortalFields } from "./PortalInspector";
import {
  createPortal,
  type Edits,
  newPortalForm,
  type PortalForm,
  portalKeys,
  portalOf,
  retryCommand,
  type Spot,
} from "./state";

interface Props {
  spot: Spot;
  /** the world's id: the new portal's level */
  level: string;
  advanced: boolean;
  worlds: readonly WorldEntry[];
  items: readonly Item[];
  itemsError?: string;
  edits: Edits;
  submit: (make: (ref: number) => Command) => number;
  retry: (ref: number) => void;
  forget: (ref: number) => void;
  /** called when the create is done, or when the author lets go of the form */
  onClose: () => void;
}

export function NewPortal({ spot, level, advanced, worlds, items, itemsError, edits, submit, retry, forget, onClose }: Props) {
  const [form, setForm] = useState<PortalForm>(() => newPortalForm(spot));
  const [pending, setPending] = useState<number>();
  const [problem, setProblem] = useState<string>();
  const mine = pending === undefined ? undefined : edits.get(pending);

  // the create is done when its edit leaves the open edits
  useEffect(() => {
    if (pending !== undefined && !edits.has(pending)) onClose();
  }, [edits, pending, onClose]);

  const apply = () => {
    const built = portalOf(form, { level, key: null }, advanced);
    if (!built.ok) {
      setProblem(built.error);
      return;
    }
    // a new try after a refusal is a new edit: let go of the old one first
    if (pending !== undefined) forget(pending);
    setProblem(undefined);
    const portal = built.value;
    setPending(submit((ref) => createPortal(portal, ref)));
  };

  const busy = mine?.status === "sent";

  return (
    <Paper withBorder shadow="sm" p="sm" role="dialog" aria-label="New portal">
      <Stack gap="sm">
        <Group justify="space-between" wrap="nowrap">
          <Text fw={700} fz="sm">
            New portal
          </Text>
          <Button size="xs" variant="subtle" onClick={onClose}>
            Close
          </Button>
        </Group>
        <PortalFields
          form={form}
          advanced={advanced}
          nameLocked={false}
          worlds={worlds}
          keys={portalKeys(items, null)}
          itemsError={itemsError}
          disabled={busy}
          onChange={setForm}
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
          {busy ? "Sending…" : "Create portal"}
        </Button>
      </Stack>
    </Paper>
  );
}
