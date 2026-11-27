// The inspector for a selected prop: its look, state, readable text and (Advanced) channel, with
// Apply and Remove. Each change is one `set` or `remove` with the seq the form OPENED at (so a
// change made since then is refused as Changed, not overwritten) and a fresh ref. The form follows
// its own ref, as the portal inspector does: it closes nothing, but shows the client's reason when
// a change is refused. A Changed refusal offers a Reload, which takes the prop as it now is.
// The server decides who may change a prop (a Basic editor only in their own plot); its refusal
// is shown as it comes.

import { Alert, Button, Group, Select, Stack, Text, TextInput } from "@mantine/core";
import { useMemo, useState } from "react";
import type { Prop } from "../../game/editor";
import { EditNotice, type Submit, useAdoptSaved, useEditFollow } from "./follow";
import { type Edits, propFormOf, type PropForm, propOf, removeEntity, setEntity } from "./state";

/** The codes a look or a state can take (0 to 7), as select options. */
const CODES = ["0", "1", "2", "3", "4", "5", "6", "7"].map((c) => ({ value: c, label: c }));

interface Props {
  entity: number;
  /** the seq the page holds for this entity now: what a reload takes */
  seq: number;
  prop: Prop;
  /** the prop's name in the catalogue, or the unknown-prop label */
  name: string;
  advanced: boolean;
  edits: Edits;
  submit: Submit;
  retry: (ref: number) => void;
  forget: (ref: number) => void;
}

export function PropInspector({ entity, seq, prop, name, advanced, edits, submit, retry, forget }: Props) {
  const [form, setForm] = useState<PropForm>(() => propFormOf(prop));
  // the seq this form expects: what Apply and Remove send, so a change made since is refused as
  // Changed rather than overwritten. After this form's own Apply or Remove goes through, it takes
  // the seq the prop now has (useAdoptSaved).
  const [expectSeq, setExpectSeq] = useState<number>(seq);
  const follow = useEditFollow(edits, submit, forget);
  const current = useMemo(() => ({ seq }), [seq]);
  useAdoptSaved(current, expectSeq, follow.doneTick, (r) => setExpectSeq(r.seq));

  const apply = () => {
    const built = propOf(form, prop, advanced);
    if (!built.ok) return follow.setProblem(built.error);
    const next = built.value;
    follow.start((ref) => setEntity(entity, { Prop: next }, expectSeq, ref));
  };

  const remove = () => follow.start((ref) => removeEntity(entity, expectSeq, ref));

  // takes the prop as it now is, and the seq it now has, after a Changed refusal
  const reload = () => {
    follow.clear();
    setForm(propFormOf(prop));
    setExpectSeq(seq);
  };

  const busy = follow.busy;
  const set = (patch: Partial<PropForm>) => setForm((f) => ({ ...f, ...patch }));

  return (
    <Stack gap="sm">
      <Text fw={700} fz="sm">
        Prop
      </Text>
      <Text fz="sm">{name}</Text>
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
      {follow.problem && (
        <Alert color="ember" role="alert">
          {follow.problem}
        </Alert>
      )}
      <EditNotice edits={edits} pending={follow.pending} retry={retry} reload={reload} />
      <Group gap="xs">
        <Button size="sm" onClick={apply} disabled={busy}>
          {busy ? "Sending…" : "Apply"}
        </Button>
        <Button size="sm" variant="default" color="ember" onClick={remove} disabled={busy}>
          Remove
        </Button>
      </Group>
    </Stack>
  );
}
