// The inspector for a selected graph node (Advanced only): its kind, how far it is refined, its
// footprint, its recipe (JSON, in a monospace box) and its seed, with Apply and Remove, and a Build
// button that asks the server to build the node. Apply and Remove are one `set` or `remove` with the
// seq the form OPENED at (a change made since is refused as Changed, not overwritten) and a fresh
// ref, followed as the prop inspector follows its own. Build is not an edit: it has no ref, and a
// refusal of it (say, TooBig, with the reason) comes back on the page's refusal line.

import { Alert, Button, Group, Select, Stack, Text, Textarea, TextInput } from "@mantine/core";
import { useMemo, useState } from "react";
import { messageOf } from "../../api/client";
import { type Node, sendCommand } from "../../game/editor";
import { TripleFields } from "./SpawnObjective";
import { EditNotice, type Submit, useAdoptSaved, useEditFollow } from "./follow";
import { type Edits, type NodeForm, nodeFormOf, nodeOf, removeEntity, setEntity } from "./state";

/** How far a node may be refined, 0 to 4, as select options. */
const DETAILS = ["0", "1", "2", "3", "4"].map((d) => ({ value: d, label: d }));

interface Props {
  entity: number;
  /** the seq the page holds for this node now: what a reload takes */
  seq: number;
  node: Node;
  /** the block count the client reports for this node, once it does; until then Build says just Build */
  blocks?: number;
  edits: Edits;
  submit: Submit;
  retry: (ref: number) => void;
  forget: (ref: number) => void;
}

export function NodeInspector({ entity, seq, node, blocks, edits, submit, retry, forget }: Props) {
  const [form, setForm] = useState<NodeForm>(() => nodeFormOf(node));
  // the seq this form expects: what Apply and Remove send (see PropInspector)
  const [expectSeq, setExpectSeq] = useState<number>(seq);
  const follow = useEditFollow(edits, submit, forget);
  const current = useMemo(() => ({ seq }), [seq]);
  useAdoptSaved(current, expectSeq, follow.doneTick, (r) => setExpectSeq(r.seq));
  const [buildProblem, setBuildProblem] = useState<string>();

  const apply = () => {
    const built = nodeOf(form, node.parent);
    if (!built.ok) return follow.setProblem(built.error);
    const next = built.value;
    follow.start((ref) => setEntity(entity, { Node: next }, expectSeq, ref));
  };

  const remove = () => follow.start((ref) => removeEntity(entity, expectSeq, ref));

  // builds the node as the server holds it; a refusal comes back on the page's refusal line
  const build = () => {
    setBuildProblem(undefined);
    try {
      sendCommand({ type: "build", node: entity });
    } catch (err) {
      setBuildProblem(messageOf(err));
    }
  };

  // takes the node as it now is, and the seq it now has, after a Changed refusal
  const reload = () => {
    follow.clear();
    setForm(nodeFormOf(node));
    setExpectSeq(seq);
  };

  const busy = follow.busy;
  const set = (patch: Partial<NodeForm>) => setForm((f) => ({ ...f, ...patch }));

  return (
    <Stack gap="sm">
      <Text fw={700} fz="sm">
        Node
      </Text>
      <TextInput
        label="Kind"
        description="A name of lower-case letters, digits and underscores, such as house"
        value={form.kind}
        disabled={busy}
        onChange={(e) => set({ kind: e.currentTarget.value })}
      />
      <Select
        label="Detail"
        description="How far it is refined: 0 is intent, 4 is dressing"
        data={DETAILS}
        value={form.detail}
        allowDeselect={false}
        disabled={busy}
        onChange={(v) => v && set({ detail: v })}
      />
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
      <Textarea
        label="Recipe (JSON, optional)"
        description="At most 16 KiB. The server checks it fully when the node is built"
        autosize
        minRows={4}
        ff="monospace"
        value={form.recipe}
        disabled={busy}
        onChange={(e) => set({ recipe: e.currentTarget.value })}
      />
      <TextInput
        label="Seed"
        description="A whole number from 0 to 9007199254740991; the same seed builds the same way"
        inputMode="numeric"
        value={form.seed}
        disabled={busy}
        onChange={(e) => set({ seed: e.currentTarget.value })}
      />
      {follow.problem && (
        <Alert color="ember" role="alert">
          {follow.problem}
        </Alert>
      )}
      {buildProblem && (
        <Alert color="ember" role="alert">
          {buildProblem}
        </Alert>
      )}
      <EditNotice edits={edits} pending={follow.pending} retry={retry} reload={reload} />
      <Group gap="xs">
        <Button size="sm" onClick={apply} disabled={busy}>
          {busy ? "Sending…" : "Apply"}
        </Button>
        <Button size="sm" variant="default" onClick={build} disabled={busy}>
          {blocks === undefined ? "Build" : `Build (${blocks} blocks)`}
        </Button>
        <Button size="sm" variant="default" color="ember" onClick={remove} disabled={busy}>
          Remove
        </Button>
      </Group>
      {/* the server builds the node it holds, so a change not yet applied is not in the build */}
      <Text size="xs" c="dimmed">
        Build uses the node as last applied: apply your changes first.
      </Text>
    </Stack>
  );
}
