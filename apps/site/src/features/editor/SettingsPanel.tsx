// The world settings panel (advanced): the level's sky, time, weather, clouds, relief, water,
// hotbar and, on an atlas tile, the edges that lead on. It edits a copy of the Settings entity's
// JSON, as the JSON the form opened on, so every key it does not show (portals, cameras, anything
// unknown) is written back as it was. Apply is one `set` that expects the seq the form opened at
// (a Changed refusal offers Reload, as the portal inspector does), or one `create` when the world
// has no Settings yet.

import {
  Alert,
  Autocomplete,
  Button,
  Checkbox,
  Group,
  MultiSelect,
  Select,
  Stack,
  Switch,
  Text,
  TextInput,
  Textarea,
} from "@mantine/core";
import { useState } from "react";
import type { Command, Component } from "../../game/editor";
import { EditNotice, TripleFields, useAdoptSaved, useEditFollow } from "./SpawnObjective";
import {
  atlasOf,
  CUSTOM_WEATHER,
  createEntity,
  type Edge,
  type Edits,
  MAX_HOTBAR,
  possibleEdges,
  type RowOf,
  setEntity,
  type SettingsForm,
  settingsFormOf,
  settingsJsonOf,
  settingsOf,
  withEdge,
} from "./state";

/** The skies the levels use (data/levels/*.json); a sky may be another name. */
const SKIES = ["void", "earth", "islands", "sea", "orbit"];
/** The weather presets the levels use; the world may name another. */
const WEATHER_PRESETS = ["none", "luminous"];
/** The four edges, in the order the form lists them. */
const EDGE_NAMES: Edge[] = ["east", "west", "south", "north"];

interface Props {
  /** the world's Settings entity, when it has one */
  row?: RowOf<"Settings">;
  /** the world's block names (from `ready`), for the hotbar */
  palette: string[];
  edits: Edits;
  submit: (make: (ref: number) => Command) => number;
  retry: (ref: number) => void;
  forget: (ref: number) => void;
  /** closes the panel, when it was opened by the button rather than by a selected entity */
  onClose?: () => void;
}

export function SettingsPanel({ row, palette, edits, submit, retry, forget, onClose }: Props) {
  // the JSON the form opened on and the seq it expects: what Apply writes over and checks against
  const [opened, setOpened] = useState(() => ({
    seq: row?.seq ?? 0,
    json: row?.component.Settings.json ?? "{}",
  }));
  const base = settingsOf(opened.json);
  const [form, setForm] = useState<SettingsForm>(() => settingsFormOf(base.ok ? base.value : {}));
  const follow = useEditFollow(edits, submit, forget);
  const busy = follow.busy;
  const set = (patch: Partial<SettingsForm>) => setForm((f) => ({ ...f, ...patch }));
  // after this form's own Apply, what Apply writes over and expects is the saved row's
  useAdoptSaved(row, opened.seq, follow.doneTick, (r) =>
    setOpened({ seq: r.seq, json: r.component.Settings.json }),
  );

  const apply = () => {
    if (!base.ok) return follow.setProblem(base.error);
    const built = settingsJsonOf(base.value, form);
    if (!built.ok) return follow.setProblem(built.error);
    const component: Component = { Settings: { json: built.value } };
    follow.start((ref) => (row ? setEntity(row.entity, component, opened.seq, ref) : createEntity(component, ref)));
  };

  // takes the settings as they now are, and the seq they now have, after a Changed refusal. With no
  // row now there is nothing to take: the refusal is cleared and the form keeps what was typed (a
  // row that appears remounts the panel, see EditorPage, so it opens on that row)
  const reload = () => {
    follow.clear();
    if (!row) return;
    setOpened({ seq: row.seq, json: row.component.Settings.json });
    const next = settingsOf(row.component.Settings.json);
    setForm(settingsFormOf(next.ok ? next.value : {}));
  };

  const atlas = base.ok ? atlasOf(base.value) : null;
  const edgesHere = atlas ? possibleEdges(atlas.tile, atlas.size) : [];
  const weatherOptions = [...WEATHER_PRESETS];
  if (form.weather !== "" && form.weather !== CUSTOM_WEATHER && !weatherOptions.includes(form.weather)) {
    weatherOptions.push(form.weather);
  }
  const blockOptions = [...new Set([...palette, ...form.hotbar])];
  const unknownBlocks = palette.length > 0 ? form.hotbar.filter((b) => !palette.includes(b)) : [];
  const water = form.water;

  return (
    <Stack gap="xs">
      <Text fw={700} fz="sm">
        World settings
      </Text>
      {!base.ok && (
        <Alert color="ember" role="alert">
          {base.error}
        </Alert>
      )}
      <Autocomplete
        label="Sky"
        description="One of the skies, or another name"
        data={SKIES}
        value={form.sky}
        disabled={busy}
        onChange={(v) => set({ sky: v })}
      />
      <TextInput
        label="Time (hours, 0 to 24)"
        inputMode="decimal"
        value={form.time}
        disabled={busy}
        onChange={(e) => set({ time: e.currentTarget.value })}
      />
      <Select
        label="Weather"
        data={[
          { value: "", label: "Default" },
          ...weatherOptions.map((w) => ({ value: w, label: w })),
          { value: CUSTOM_WEATHER, label: "Custom (JSON)" },
        ]}
        value={form.weather}
        allowDeselect={false}
        disabled={busy}
        onChange={(v) => v !== null && set({ weather: v })}
      />
      {form.weather === CUSTOM_WEATHER && (
        <Textarea
          label="Weather as a JSON object"
          autosize
          minRows={4}
          ff="monospace"
          value={form.weatherJson}
          disabled={busy}
          onChange={(e) => set({ weatherJson: e.currentTarget.value })}
        />
      )}
      <TextInput
        label="Clouds (0 to 1, empty to leave out)"
        inputMode="decimal"
        value={form.clouds}
        disabled={busy}
        onChange={(e) => set({ clouds: e.currentTarget.value })}
      />
      <TextInput
        label="Relief (0 to 1, empty to leave out)"
        inputMode="decimal"
        value={form.relief}
        disabled={busy}
        onChange={(e) => set({ relief: e.currentTarget.value })}
      />
      <Switch
        label="Water"
        checked={water !== null}
        disabled={busy}
        onChange={(e) =>
          set({ water: e.currentTarget.checked ? { colour: ["", "", ""], swell: "", opacity: "" } : null })
        }
      />
      {water && (
        <Stack gap="xs">
          <TripleFields
            label="Colour (red, green, blue, 0 to 1)"
            names={["r", "g", "b"]}
            values={water.colour}
            disabled={busy}
            onChange={(i, v) => {
              const colour = [...water.colour] as [string, string, string];
              colour[i] = v;
              set({ water: { ...water, colour } });
            }}
          />
          <TextInput
            label="Swell (metres, 0 to 3)"
            inputMode="decimal"
            value={water.swell}
            disabled={busy}
            onChange={(e) => set({ water: { ...water, swell: e.currentTarget.value } })}
          />
          <TextInput
            label="Opacity (0 to 1)"
            inputMode="decimal"
            value={water.opacity}
            disabled={busy}
            onChange={(e) => set({ water: { ...water, opacity: e.currentTarget.value } })}
          />
        </Stack>
      )}
      <Stack gap={4}>
        <MultiSelect
          label={`Hotbar (up to ${MAX_HOTBAR} blocks)`}
          data={blockOptions}
          value={form.hotbar}
          maxValues={MAX_HOTBAR}
          searchable
          disabled={busy}
          onChange={(v) => set({ hotbar: v })}
        />
        {unknownBlocks.length > 0 && (
          <Text fz="xs" c="ember">
            Not among this world's blocks (kept as they are): {unknownBlocks.join(", ")}
          </Text>
        )}
      </Stack>
      {atlas && (
        <Stack gap={4}>
          <Text fz="sm" fw={500}>
            Edges that lead on
          </Text>
          <Text fz="xs" c="moon.5">
            A closed edge refuses walking through to the next tile.
          </Text>
          {EDGE_NAMES.filter((e) => edgesHere.includes(e)).map((e) => (
            <Checkbox
              key={e}
              label={e}
              checked={form.edges.includes(e)}
              disabled={busy}
              onChange={(ev) => set({ edges: withEdge(form.edges, e, ev.currentTarget.checked, edgesHere) })}
            />
          ))}
        </Stack>
      )}
      {follow.problem && (
        <Alert color="ember" role="alert">
          {follow.problem}
        </Alert>
      )}
      <EditNotice edits={edits} pending={follow.pending} retry={retry} reload={reload} />
      <Group gap="xs">
        <Button size="sm" onClick={apply} disabled={busy || !base.ok}>
          {busy ? "Sending…" : "Apply"}
        </Button>
        {onClose && (
          <Button size="sm" variant="default" onClick={onClose} disabled={busy}>
            Close
          </Button>
        )}
      </Group>
    </Stack>
  );
}
