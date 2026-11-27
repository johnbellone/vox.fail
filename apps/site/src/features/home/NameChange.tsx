// Changing your name from the start page: a link that opens a small form. Refusals show as text.

import { Anchor, Button, Group, Stack, Text, TextInput } from "@mantine/core";
import { useState } from "react";
import { messageOf, post, when } from "../../api/client";
import type { Me } from "../../api/types";

/** The note under the form about when the name may next change, or undefined when it may already. */
export function renameNote(renameAfter: number | null | undefined, nowSeconds: number): string | undefined {
  if (renameAfter == null || renameAfter <= nowSeconds) return undefined;
  return `You can change your name again on ${when(renameAfter)}. Changing capitals only is fine any time.`;
}

export function NameChange({ me, onChanged }: { me: Me; onChanged: () => void }) {
  const [open, setOpen] = useState(false);
  const [name, setName] = useState(me.name ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();

  if (!open) {
    return (
      <Anchor component="button" type="button" fz="sm" onClick={() => {
          setName(me.name ?? "");
          setOpen(true);
        }}
      >
        Change your name
      </Anchor>
    );
  }

  const note = renameNote(me.rename_after, Date.now() / 1000);

  return (
    <form
      onSubmit={async (e) => {
        e.preventDefault();
        setBusy(true);
        setError(undefined);
        try {
          await post("/api/name", { name: name.trim() });
          setOpen(false);
          onChanged();
        } catch (err) {
          setError(messageOf(err));
        } finally {
          setBusy(false);
        }
      }}
    >
      <Stack gap="xs">
        <TextInput
          label="New name"
          value={name}
          onChange={(e) => setName(e.currentTarget.value)}
          maxLength={20}
          autoComplete="off"
          required
          error={error}
        />
        {note && (
          <Text c="moon.5" fz="xs">
            {note}
          </Text>
        )}
        <Group gap="xs">
          <Button type="submit" size="xs" loading={busy}>
            Save
          </Button>
          <Button
            type="button"
            size="xs"
            variant="default"
            onClick={() => {
              setOpen(false);
              setError(undefined);
              setName(me.name ?? "");
            }}
          >
            Cancel
          </Button>
        </Group>
      </Stack>
    </form>
  );
}
