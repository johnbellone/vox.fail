// The inspector for a selected portal: its fields, its destination, and Apply and Remove. Each
// change is one `set` or `remove` with the seq the form OPENED at (so a change made since then is
// refused as Changed, not overwritten) and a fresh ref. The form follows its own ref: it closes
// when that edit is done, and shows the client's reason when it is refused. A Changed refusal
// offers a Reload, which takes the portal as it now is and the seq it now has. The destination
// fields are shared with the new-portal form.

import { Alert, Button, Group, Select, Stack, Text, TextInput } from "@mantine/core";
import { useMemo, useState } from "react";
import type { Item, WorldEntry } from "../../api/types";
import type { Portal } from "../../game/editor";
import { EditNotice, type Submit, useAdoptSaved, useEditFollow } from "./follow";
import {
  BASIC_SIZE_HINT,
  destKinds,
  type DestForm,
  type DestKind,
  type Edits,
  portalFormOf,
  type PortalForm,
  type PortalKey,
  portalKeys,
  portalOf,
  reloaded,
  removePortal,
  rightsFor,
  setPortal,
  YAWS,
} from "./state";

const KIND_LABEL: Record<DestKind, string> = {
  World: "a world",
  Pair: "my other portal",
  Here: "here",
  Url: "url",
  Dungeon: "dungeon",
};

/** The turns the Turn select offers: the quarter turns, and the form's own turn when it is another
 *  angle (a level file may turn a portal to any angle), so the select can show it. */
function turnChoices(yaw: string): string[] {
  const quarters = YAWS.map(String);
  return quarters.includes(yaw) ? quarters : [...quarters, yaw];
}

/** Three or two text fields in a row, one per number. */
function NumberFields({
  label,
  names,
  values,
  disabled,
  onChange,
}: {
  label: string;
  names: readonly string[];
  values: readonly string[];
  disabled: boolean;
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

/** The destination picker: the kind, then what that kind needs. The kinds offered depend on the
 *  mode; `itemsError` is shown when the player's portal keys could not be loaded. */
export function DestinationFields({
  value,
  advanced,
  worlds,
  keys,
  itemsError,
  disabled,
  onChange,
}: {
  value: DestForm;
  advanced: boolean;
  worlds: readonly WorldEntry[];
  keys: readonly PortalKey[];
  itemsError?: string;
  disabled: boolean;
  onChange: (d: DestForm) => void;
}) {
  const set = (patch: Partial<DestForm>) => onChange({ ...value, ...patch });
  const kinds = destKinds(advanced, value.kind);
  // an existing pair whose key is not among the player's keys still shows, as its number
  const pairKeys: PortalKey[] =
    value.key && !keys.some((k) => String(k.key) === value.key)
      ? [...keys, { key: Number(value.key), label: `Portal ${value.key}` }]
      : [...keys];
  const worldData = worlds.map((w) => ({ value: w.id, label: w.name }));
  const arrive = (
    <NumberFields
      label="Arrive at"
      names={["x", "y", "z"]}
      values={value.arrive}
      disabled={disabled}
      onChange={(i, v) => {
        const next = [...value.arrive] as [string, string, string];
        next[i] = v;
        set({ arrive: next });
      }}
    />
  );
  return (
    <Stack gap="xs">
      <Select
        label="Leads to"
        data={kinds.map((k) => ({ value: k, label: KIND_LABEL[k] }))}
        value={value.kind}
        allowDeselect={false}
        disabled={disabled}
        onChange={(v) => v && set({ kind: v as DestKind })}
      />
      {value.kind === "World" && (
        <Select
          label="World"
          placeholder="Choose a world"
          searchable
          data={worldData}
          value={value.world || null}
          disabled={disabled}
          onChange={(v) => set({ world: v ?? "" })}
        />
      )}
      {value.kind === "Pair" && (
        <Stack gap={4}>
          <Select
            label="Portal"
            placeholder="Choose one of your portals"
            data={pairKeys.map((k) => ({ value: String(k.key), label: k.label }))}
            value={value.key || null}
            disabled={disabled}
            onChange={(v) => set({ key: v ?? "" })}
          />
          {keys.length === 0 && !itemsError && (
            <Text fz="xs" c="moon.5">
              You have no other portals yet.
            </Text>
          )}
          {itemsError && (
            <Text fz="xs" c="ember">
              Could not load your portals: {itemsError}
            </Text>
          )}
        </Stack>
      )}
      {value.kind === "Url" && (
        <>
          <TextInput
            label="Address"
            placeholder=":7781 or /w/7781/ws"
            value={value.url}
            disabled={disabled}
            onChange={(e) => set({ url: e.currentTarget.value })}
          />
          {arrive}
        </>
      )}
      {value.kind === "Dungeon" && (
        <>
          <TextInput
            label="Dungeon"
            placeholder="its name"
            value={value.dungeon}
            disabled={disabled}
            onChange={(e) => set({ dungeon: e.currentTarget.value })}
          />
          {arrive}
        </>
      )}
      {value.kind === "Here" && arrive}
    </Stack>
  );
}

/** The portal fields (name, place, axis, turn, size) and destination, shared with the new-portal
 *  form. `nameLocked` keeps the name read-only (a player's portal's name belongs to its owner). */
export function PortalFields({
  form,
  advanced,
  nameLocked,
  worlds,
  keys,
  itemsError,
  disabled,
  onChange,
}: {
  form: PortalForm;
  advanced: boolean;
  nameLocked: boolean;
  worlds: readonly WorldEntry[];
  keys: readonly PortalKey[];
  itemsError?: string;
  disabled: boolean;
  onChange: (f: PortalForm) => void;
}) {
  const set = (patch: Partial<PortalForm>) => onChange({ ...form, ...patch });
  return (
    <Stack gap="xs">
      <TextInput
        label="Name"
        value={form.name}
        disabled={disabled || nameLocked}
        onChange={(e) => set({ name: e.currentTarget.value })}
      />
      <NumberFields
        label="Position"
        names={["x", "y", "z"]}
        values={form.at}
        disabled={disabled}
        onChange={(i, v) => {
          const at = [...form.at] as [string, string, string];
          at[i] = v;
          set({ at });
        }}
      />
      <Group grow gap="xs">
        <Select
          label="Axis"
          data={[
            { value: "X", label: "X" },
            { value: "Z", label: "Z" },
          ]}
          value={form.axis}
          allowDeselect={false}
          disabled={disabled}
          onChange={(v) => v && set({ axis: v as PortalForm["axis"] })}
        />
        <Select
          label="Turn"
          // a portal turned to some other angle shows that angle, so Apply keeps it
          data={turnChoices(form.yaw).map((d) => ({ value: d, label: `${d}°` }))}
          value={form.yaw}
          allowDeselect={false}
          disabled={disabled}
          onChange={(v) => v && set({ yaw: v })}
        />
      </Group>
      <Stack gap={4}>
        <NumberFields
          label="Size (width, height)"
          names={["w", "h"]}
          values={form.size}
          disabled={disabled}
          onChange={(i, v) => {
            const size = [...form.size] as [string, string];
            size[i] = v;
            set({ size });
          }}
        />
        {!advanced && (
          <Text fz="xs" c="moon.5">
            {BASIC_SIZE_HINT}
          </Text>
        )}
      </Stack>
      <DestinationFields
        value={form.dest}
        advanced={advanced}
        worlds={worlds}
        keys={keys}
        itemsError={itemsError}
        disabled={disabled}
        onChange={(dest) => set({ dest })}
      />
    </Stack>
  );
}

interface Props {
  entity: number;
  /** the seq the page holds for this entity now: what a reload takes */
  seq: number;
  portal: Portal;
  advanced: boolean;
  worlds: readonly WorldEntry[];
  items: readonly Item[];
  itemsError?: string;
  edits: Edits;
  submit: Submit;
  retry: (ref: number) => void;
  forget: (ref: number) => void;
}

export function PortalInspector({ entity, seq, portal, advanced, worlds, items, itemsError, edits, submit, retry, forget }: Props) {
  const keyed = portal.key !== null;
  const rights = rightsFor(advanced, keyed);
  const [form, setForm] = useState<PortalForm>(() => portalFormOf(portal));
  // the seq this form expects (the form is remounted per entity, by its key): what Apply and
  // Remove send, so a change made since is refused as Changed rather than overwritten. After this
  // form's own Apply or Remove goes through, it takes the seq the portal now has (useAdoptSaved).
  const [expectSeq, setExpectSeq] = useState<number>(seq);
  const follow = useEditFollow(edits, submit, forget);
  const current = useMemo(() => ({ seq }), [seq]);
  useAdoptSaved(current, expectSeq, follow.doneTick, (r) => setExpectSeq(r.seq));
  const keys = portalKeys(items, portal.key);

  const apply = () => {
    const built = portalOf(form, { level: portal.level, key: portal.key, yaw: portal.yaw }, advanced);
    if (!built.ok) return follow.setProblem(built.error);
    const portalNow = built.value;
    follow.start((ref) => setPortal(entity, portalNow, expectSeq, ref));
  };

  const remove = () => follow.start((ref) => removePortal(entity, expectSeq, ref));

  // takes the portal as it now is, and the seq it now has, after a Changed refusal
  const reload = () => {
    follow.clear();
    const next = reloaded(portal, seq);
    setForm(next.form);
    setExpectSeq(next.expect);
  };

  const busy = follow.busy;
  const disabled = !rights.editable;

  return (
    <Stack gap="sm">
      <Text fw={700} fz="sm">
        {keyed ? "A player's portal" : "Portal"}
      </Text>
      {keyed && (
        <Text fz="xs" c="moon.5">
          {advanced
            ? "Editors can't change a player's portal. Its name and where it leads belong to its owner."
            : "Its name belongs to its owner."}
        </Text>
      )}
      {!keyed && !rights.editable && (
        <Text fz="xs" c="moon.5">
          Only an Advanced editor can change a level's portal.
        </Text>
      )}
      <PortalFields
        form={form}
        advanced={advanced}
        nameLocked={!rights.nameEditable}
        worlds={worlds}
        keys={keys}
        itemsError={itemsError}
        disabled={disabled}
        onChange={setForm}
      />
      {follow.problem && (
        <Alert color="ember" role="alert">
          {follow.problem}
        </Alert>
      )}
      <EditNotice edits={edits} pending={follow.pending} retry={retry} reload={reload} />
      {rights.editable && (
        <Group gap="xs">
          <Button size="sm" onClick={apply} disabled={busy}>
            {busy ? "Sending…" : "Apply"}
          </Button>
          <Button size="sm" variant="default" color="ember" onClick={remove} disabled={busy}>
            Remove
          </Button>
        </Group>
      )}
    </Stack>
  );
}
