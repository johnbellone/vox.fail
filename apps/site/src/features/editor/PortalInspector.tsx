// The inspector for a selected portal: its fields, its destination, and Apply and Remove. Each
// change is one `set` or `remove` with the seq the form OPENED at (so a change made since then is
// refused as Changed, not overwritten) and a fresh ref. The form follows its own ref: it closes
// when that edit is done, and shows the client's reason when it is refused. A Changed refusal
// offers a Reload, which takes the portal as it now is and the seq it now has. The destination
// fields are shared with the new-portal form.

import { Alert, Button, Group, Select, Stack, Text, TextInput } from "@mantine/core";
import { useEffect, useState } from "react";
import type { Item, WorldEntry } from "../../api/types";
import type { Command, Portal } from "../../game/editor";
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
  refusalLine,
  reloaded,
  removePortal,
  retryCommand,
  rightsFor,
  setPortal,
} from "./state";

const KIND_LABEL: Record<DestKind, string> = {
  World: "a world",
  Pair: "my other portal",
  Here: "here",
  Url: "url",
  Dungeon: "dungeon",
};

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
          data={["0", "90", "180", "270"].map((d) => ({ value: d, label: `${d}°` }))}
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
  submit: (make: (ref: number) => Command) => number;
  retry: (ref: number) => void;
  forget: (ref: number) => void;
}

export function PortalInspector({ entity, seq, portal, advanced, worlds, items, itemsError, edits, submit, retry, forget }: Props) {
  const keyed = portal.key !== null;
  const rights = rightsFor(advanced, keyed);
  const [form, setForm] = useState<PortalForm>(() => portalFormOf(portal));
  // the seq this form opened at (the form is remounted per entity, by its key): what Apply and
  // Remove expect, so a change made since is refused as Changed rather than overwritten
  const [expectSeq, setExpectSeq] = useState<number>(seq);
  const [pending, setPending] = useState<number>();
  const [problem, setProblem] = useState<string>();
  const keys = portalKeys(items, portal.key);

  // the edit this form follows: done takes it out (the form lets go), a refusal stays with its text
  const mine = pending === undefined ? undefined : edits.get(pending);
  useEffect(() => {
    if (pending !== undefined && !edits.has(pending)) setPending(undefined);
  }, [edits, pending]);

  const start = (make: (ref: number) => Command) => {
    if (pending !== undefined) forget(pending);
    setProblem(undefined);
    setPending(submit(make));
  };

  const apply = () => {
    const built = portalOf(form, { level: portal.level, key: portal.key }, advanced);
    if (!built.ok) {
      setProblem(built.error);
      return;
    }
    const portalNow = built.value;
    start((ref) => setPortal(entity, portalNow, expectSeq, ref));
  };

  const remove = () => start((ref) => removePortal(entity, expectSeq, ref));

  // takes the portal as it now is, and the seq it now has, after a Changed refusal
  const reload = () => {
    if (pending !== undefined) forget(pending);
    setPending(undefined);
    setProblem(undefined);
    const next = reloaded(portal, seq);
    setForm(next.form);
    setExpectSeq(next.expect);
  };

  const busy = mine?.status === "sent";
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
      {problem && (
        <Alert color="ember" role="alert">
          {problem}
        </Alert>
      )}
      {mine?.status === "refused" && pending !== undefined && (
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
      )}
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
