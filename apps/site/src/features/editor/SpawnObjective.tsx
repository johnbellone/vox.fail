// The forms for a world's Spawn (where players start) and Objective (a place to reach), in the
// advanced editor. Each is one `set` or `remove` on the entity it edits, with the seq the form
// opened at (so a change made since is refused as Changed, as the portal inspector does), or one
// `create` when the world has none yet. The position is typed, or copied from the marked spot.
// Also the pieces the settings form shares: the notice for one open edit, a retry or reload, and
// the three-field number row.

import { Alert, Button, Group, Stack, Text, TextInput } from "@mantine/core";
import { useEffect, useRef, useState } from "react";
import type { Command, Component } from "../../game/editor";
import {
  createEntity,
  type Edits,
  objectiveFormOf,
  objectiveOf,
  type ObjectiveForm,
  refusalLine,
  removeEntity,
  retryCommand,
  type RowOf,
  type Spot,
  setEntity,
  spawnFormOf,
  spawnOf,
  type SpawnForm,
  textOf,
} from "./state";

type Submit = (make: (ref: number) => Command) => number;

/** One panel's edit, followed by its ref: the open edit, its refusal, and whether it is sending.
 *  A done edit lets go (the panel is free again); a refusal stays with its text. */
export function useEditFollow(edits: Edits, submit: Submit, forget: (ref: number) => void) {
  const [pending, setPending] = useState<number>();
  const [problem, setProblem] = useState<string>();
  // counts the edits this panel saw go through (the client's `done`), so the panel can take the
  // seq the world now has (see useAdoptSaved)
  const [doneTick, setDoneTick] = useState(0);
  useEffect(() => {
    if (pending !== undefined && !edits.has(pending)) {
      setPending(undefined);
      setDoneTick((t) => t + 1);
    }
  }, [edits, pending]);

  const start = (make: (ref: number) => Command) => {
    if (pending !== undefined) forget(pending);
    setProblem(undefined);
    setPending(submit(make));
  };
  // lets go of the open edit and any problem, after a reload or before a new form
  const clear = () => {
    if (pending !== undefined) forget(pending);
    setPending(undefined);
    setProblem(undefined);
  };
  const mine = pending === undefined ? undefined : edits.get(pending);
  return { pending, problem, setProblem, busy: mine?.status === "sent", doneTick, start, clear };
}

/** After this panel's own edit goes through, the form expects the seq the row now has, so a second
 *  Apply is not refused as Changed. Waits for the row to show a seq other than the one the form
 *  holds (the `done` may arrive before the row does), then calls `adopt` with the row. A change by
 *  someone else, with no edit of ours settled, is not taken. */
export function useAdoptSaved<R extends { seq: number }>(
  row: R | undefined,
  seq: number,
  doneTick: number,
  adopt: (row: R) => void,
) {
  const seenDone = useRef(0);
  const waiting = useRef(false);
  useEffect(() => {
    if (doneTick > seenDone.current) {
      seenDone.current = doneTick;
      waiting.current = true;
    }
    if (!waiting.current || !row || row.seq === seq) return;
    waiting.current = false;
    adopt(row);
  }, [row, seq, doneTick]);
}

/** The refusal of a panel's open edit: its line, a Reload after a Changed refusal, and a retry
 *  after one the client calls Unavailable (the same ref, so nothing is charged twice). */
export function EditNotice({
  edits,
  pending,
  retry,
  reload,
}: {
  edits: Edits;
  pending: number | undefined;
  retry: (ref: number) => void;
  reload: () => void;
}) {
  const mine = pending === undefined ? undefined : edits.get(pending);
  if (pending === undefined || mine?.status !== "refused") return null;
  return (
    <Alert color="ember" role="alert">
      <Stack gap="xs">
        <Text fz="sm">{refusalLine(mine)}</Text>
        {mine.why === "Changed" ? (
          <Button size="xs" variant="default" onClick={reload} w="fit-content">
            Reload
          </Button>
        ) : (
          retryCommand(edits, pending) !== undefined && (
            <Button size="xs" variant="default" onClick={() => retry(pending)} w="fit-content">
              Try again
            </Button>
          )
        )}
      </Stack>
    </Alert>
  );
}

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
        label="Radius (blocks, more than 0, at most 256)"
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
