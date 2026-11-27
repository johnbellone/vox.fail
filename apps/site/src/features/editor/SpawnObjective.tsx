// The forms for a world's Spawn (where players start) and Objective (a place to reach), in the
// advanced editor. Each is one `set` or `remove` on the entity it edits, with the seq the form
// opened at (so a change made since is refused as Changed, as the portal inspector does), or one
// `create` when the world has none yet. The position is typed, or copied from the marked spot.
// The edit-following hooks and the notice for a refused edit live in ./follow (shared with the
// portal inspector and the settings form). Also the three-field number row, which the settings
// form shares.

import { Alert, Button, Group, Stack, Text, TextInput } from "@mantine/core";
import { useState } from "react";
import type { Component } from "../../game/editor";
import { EditNotice, type Submit, useAdoptSaved, useEditFollow } from "./follow";
import {
  createEntity,
  type Edits,
  MAX_RADIUS,
  objectiveFormOf,
  objectiveOf,
  type ObjectiveForm,
  removeEntity,
  type RowOf,
  type Spot,
  setEntity,
  spawnFormOf,
  spawnOf,
  type SpawnForm,
  textOf,
} from "./state";

/** Three number fields in a row under one label. */
export function TripleFields({
  label,
  names,
  values,
  disabled,
  onChange,
}: {
  label: string;
  names: readonly string[];
  values: readonly string[];
  disabled?: boolean;
  onChange: (i: number, v: string) => void;
}) {
  return (
    <Group grow gap="xs" wrap="nowrap" align="flex-end">
      {names.map((n, i) => (
        <TextInput
          key={n}
          label={i === 0 ? label : undefined}
          aria-label={`${label} ${n}`}
          inputMode="decimal"
          value={values[i] ?? ""}
          disabled={disabled}
          onChange={(e) => onChange(i, e.currentTarget.value)}
          placeholder={n}
        />
      ))}
    </Group>
  );
}

/** A position (feet, blocks), typed or copied from the marked spot. */
function PositionFields({
  at,
  spot,
  disabled,
  onChange,
}: {
  at: [string, string, string];
  spot?: Spot;
  disabled: boolean;
  onChange: (at: [string, string, string]) => void;
}) {
  return (
    <Stack gap={4}>
      <TripleFields
        label="Position (feet, blocks)"
        names={["x", "y", "z"]}
        values={at}
        disabled={disabled}
        onChange={(i, v) => {
          const next = [...at] as [string, string, string];
          next[i] = v;
          onChange(next);
        }}
      />
      {spot && (
        <Button size="xs" variant="default" disabled={disabled} onClick={() => onChange(textOf(spot.at))} w="fit-content">
          Use marked spot
        </Button>
      )}
    </Stack>
  );
}

interface SpawnProps {
  /** the world's Spawn, when it has one */
  row?: RowOf<"Spawn">;
  /** the spot the Portal tool last marked */
  spot?: Spot;
  edits: Edits;
  submit: Submit;
  retry: (ref: number) => void;
  forget: (ref: number) => void;
  /** closes the panel, when it was opened by its button rather than by a selected entity */
  onClose?: () => void;
}

/** The form for the world's spawn point. Without one, Apply adds one. */
export function SpawnPanel({ row, spot, edits, submit, retry, forget, onClose }: SpawnProps) {
  // the seq this form expects (the panel is remounted per row, by its key)
  const [expect, setExpect] = useState<number>(row?.seq ?? 0);
  const [form, setForm] = useState<SpawnForm>(() => spawnFormOf(row?.component.Spawn, spot));
  const follow = useEditFollow(edits, submit, forget);
  useAdoptSaved(row, expect, follow.doneTick, (r) => setExpect(r.seq));

  const apply = () => {
    const built = spawnOf(form);
    if (!built.ok) return follow.setProblem(built.error);
    const component: Component = { Spawn: built.value };
    follow.start((ref) => (row ? setEntity(row.entity, component, expect, ref) : createEntity(component, ref)));
  };
  const remove = () => {
    if (row) follow.start((ref) => removeEntity(row.entity, expect, ref));
  };
  // takes the spawn as it now is, and the seq it now has, after a Changed refusal
  const reload = () => {
    follow.clear();
    if (!row) return;
    setForm(spawnFormOf(row.component.Spawn, spot));
    setExpect(row.seq);
  };

  return (
    <Stack gap="xs">
      <Text fw={700} fz="sm">
        {row ? "Spawn" : "No spawn point yet"}
      </Text>
      <PositionFields
        at={form.at}
        spot={spot}
        disabled={follow.busy}
        onChange={(at) => setForm({ ...form, at })}
      />
      <TextInput
        label="Turn (degrees)"
        inputMode="decimal"
        value={form.yaw}
        disabled={follow.busy}
        onChange={(e) => setForm({ ...form, yaw: e.currentTarget.value })}
      />
      {follow.problem && (
        <Alert color="ember" role="alert">
          {follow.problem}
        </Alert>
      )}
      <EditNotice edits={edits} pending={follow.pending} retry={retry} reload={reload} />
      <Group gap="xs">
        <Button size="sm" onClick={apply} disabled={follow.busy}>
          {follow.busy ? "Sending…" : row ? "Apply" : "Add spawn"}
        </Button>
        {row && (
          <Button size="sm" variant="default" color="ember" onClick={remove} disabled={follow.busy}>
            Remove
          </Button>
        )}
        {onClose && (
          <Button size="sm" variant="default" onClick={onClose} disabled={follow.busy}>
            Close
          </Button>
        )}
      </Group>
    </Stack>
  );
}

interface ObjectiveProps {
  /** the world's Objective, when it has one */
  row?: RowOf<"Objective">;
  spot?: Spot;
  edits: Edits;
  submit: Submit;
  retry: (ref: number) => void;
  forget: (ref: number) => void;
  /** closes the panel, when it was opened by its button rather than by a selected entity */
  onClose?: () => void;
}

/** The form for the world's objective. Without one, Apply adds one. */
export function ObjectivePanel({ row, spot, edits, submit, retry, forget, onClose }: ObjectiveProps) {
  const [expect, setExpect] = useState<number>(row?.seq ?? 0);
  const [form, setForm] = useState<ObjectiveForm>(() => objectiveFormOf(row?.component.Objective, spot));
  const follow = useEditFollow(edits, submit, forget);
  useAdoptSaved(row, expect, follow.doneTick, (r) => setExpect(r.seq));

  const apply = () => {
    const built = objectiveOf(form);
    if (!built.ok) return follow.setProblem(built.error);
    const component: Component = { Objective: built.value };
    follow.start((ref) => (row ? setEntity(row.entity, component, expect, ref) : createEntity(component, ref)));
  };
  const remove = () => {
    if (row) follow.start((ref) => removeEntity(row.entity, expect, ref));
  };
  const reload = () => {
    follow.clear();
    if (!row) return;
    setForm(objectiveFormOf(row.component.Objective, spot));
    setExpect(row.seq);
  };

  return (
    <Stack gap="xs">
      <Text fw={700} fz="sm">
        {row ? "Objective" : "No objective yet"}
      </Text>
      <TextInput
        label="Name"
        value={form.name}
        disabled={follow.busy}
        onChange={(e) => setForm({ ...form, name: e.currentTarget.value })}
      />
      <PositionFields at={form.at} spot={spot} disabled={follow.busy} onChange={(at) => setForm({ ...form, at })} />
      <TextInput
        label={`Radius (blocks, more than 0, at most ${MAX_RADIUS})`}
        inputMode="decimal"
        value={form.radius}
        disabled={follow.busy}
        onChange={(e) => setForm({ ...form, radius: e.currentTarget.value })}
      />
      {follow.problem && (
        <Alert color="ember" role="alert">
          {follow.problem}
        </Alert>
      )}
      <EditNotice edits={edits} pending={follow.pending} retry={retry} reload={reload} />
      <Group gap="xs">
        <Button size="sm" onClick={apply} disabled={follow.busy}>
          {follow.busy ? "Sending…" : row ? "Apply" : "Add objective"}
        </Button>
        {row && (
          <Button size="sm" variant="default" color="ember" onClick={remove} disabled={follow.busy}>
            Remove
          </Button>
        )}
        {onClose && (
          <Button size="sm" variant="default" onClick={onClose} disabled={follow.busy}>
            Close
          </Button>
        )}
      </Group>
    </Stack>
  );
}
