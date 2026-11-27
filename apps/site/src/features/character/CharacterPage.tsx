// Who are you in Vox: pick a character, then what to wear, and save.

import { Alert, Button, Chip, Grid, Group, Image, Loader, Paper, SimpleGrid, Stack, Text, Title, UnstyledButton } from "@mantine/core";
import { useEffect, useMemo, useState } from "react";
import { api, messageOf } from "../../api/client";
import { useApi, useSignedIn } from "../../api/hooks";
import type { CastMember, Catalogue, Slot } from "../../api/types";
import { Page } from "../../components/Page";
import { Problem } from "../../components/Problem";
import { Turntable } from "./Turntable";
import type { Worn } from "./preview";

const GROUPS: [string, string][] = [
  ["Main", "Vox's own"],
  ["Cast", "The cast"],
  ["Animals", "Animals"],
];
const SLOTS: [Slot, string][] = [
  ["hat", "On your head"],
  ["face", "On your face"],
  ["hand", "In your hand"],
];

export function CharacterPage() {
  const me = useSignedIn();
  const cat = useApi<Catalogue>("/api/catalogue");
  const [model, setModel] = useState<string>();
  const [wear, setWear] = useState<Partial<Record<Slot, string>>>({});
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string>();

  // start from what you have, or the first character
  useEffect(() => {
    if (!cat.data || !me.data?.name || model) return;
    const have = me.data.character;
    setModel(have?.model ?? cat.data.characters[0]?.model);
    const w: Partial<Record<Slot, string>> = {};
    for (const id of have?.wear ?? []) {
      const it = cat.data.wardrobe.find((x) => x.id === id);
      if (it) w[it.slot] = id;
    }
    setWear(w);
  }, [cat.data, me.data, model]);

  const who = cat.data?.characters.find((c) => c.model === model);
  const worn: Worn[] = useMemo(
    () =>
      SLOTS.flatMap(([s]) => {
        const it = cat.data?.wardrobe.find((w) => w.id === wear[s]);
        return it ? [{ slot: s, id: it.id, file: it.file }] : [];
      }),
    [cat.data, wear],
  );

  async function save() {
    setSaving(true);
    setError(undefined);
    try {
      await api("/api/character", "PUT", { model, wear: SLOTS.flatMap(([s]) => (wear[s] ? [wear[s]] : [])) });
      setSaved(true);
      setTimeout(() => (location.href = "/"), 500);
    } catch (err) {
      setError(messageOf(err));
      setSaving(false);
    }
  }

  if (cat.error || me.error) return <Page><Problem error={cat.error ?? me.error} /></Page>;
  if (!cat.data) return <Page><Loader /></Page>;

  return (
    <Page>
      <Grid gap="xl">
        <Grid.Col span={{ base: 12, md: 5 }}>
          <div style={{ position: "sticky", top: 16 }}>
            <Turntable who={who} worn={worn} />
          </div>
        </Grid.Col>
        <Grid.Col span={{ base: 12, md: 7 }}>
          <Paper>
            <Stack gap="lg">
              <div>
                <Title order={2}>Who are you in Vox?</Title>
                <Text c="moon.5">Pick someone to be, then something to wear. You can change it whenever you like.</Text>
              </div>
              {GROUPS.map(([g, label]) => {
                const families = familiesOf(cat.data!.characters.filter((c) => c.group === g));
                if (!families.length) return null;
                const open = families.find((f) => f.some((c) => c.model === model));
                return (
                  <Stack key={g} gap="xs">
                    <Title order={3} fz="md">
                      {label}
                    </Title>
                    <SimpleGrid cols={{ base: 3, xs: 4, sm: 5, lg: 6 }} spacing="xs" role="group" aria-label={label}>
                      {families.map((f) => (
                        <CastButton
                          key={f[0].model}
                          family={f}
                          picked={f === open}
                          shown={(f === open && who) || f[0]}
                          onPick={() => f !== open && setModel(f[0].model)}
                        />
                      ))}
                    </SimpleGrid>
                    {open && open.length > 1 && (
                      <Stack gap={4}>
                        <Text fz="sm" fw={700} id={`outfit-${g}`}>
                          {open[0].family_name ?? open[0].name}: outfit
                        </Text>
                        <Chip.Group multiple={false} value={model ?? ""} onChange={(v) => v && setModel(v)}>
                          <Group gap={6} role="group" aria-labelledby={`outfit-${g}`}>
                            {open.map((c) => (
                              <Chip key={c.model} value={c.model} size="sm">
                                {c.outfit ?? c.name}
                              </Chip>
                            ))}
                          </Group>
                        </Chip.Group>
                      </Stack>
                    )}
                  </Stack>
                );
              })}
              {SLOTS.map(([slot, label]) => (
                <Stack key={slot} gap="xs">
                  <Title order={3} fz="md" id={`${slot}-h`}>
                    {label}
                  </Title>
                  <Chip.Group
                    multiple={false}
                    value={wear[slot] ?? ""}
                    onChange={(v) => setWear((w) => ({ ...w, [slot]: v || undefined }))}
                  >
                    <Group gap={6} role="group" aria-labelledby={`${slot}-h`}>
                      <Chip value="" size="sm">
                        Nothing
                      </Chip>
                      {cat.data!.wardrobe
                        .filter((w) => w.slot === slot)
                        .map((w) => (
                          <Chip key={w.id} value={w.id} size="sm">
                            {w.label.replace(/^(a|an|the|some) /, "")}
                          </Chip>
                        ))}
                    </Group>
                  </Chip.Group>
                </Stack>
              ))}
              <Group>
                <Button size="md" onClick={save} loading={saving} disabled={!model}>
                  Save character
                </Button>
                {saved && (
                  <Text c="moon.5" role="status">
                    Saved.
                  </Text>
                )}
              </Group>
              {error && (
                <Alert color="ember" role="alert">
                  {error}
                </Alert>
              )}
            </Stack>
          </Paper>
        </Grid.Col>
      </Grid>
    </Page>
  );
}

/** Characters grouped by family, in catalogue order: a character without one is a family of one. */
type Family = [CastMember, ...CastMember[]];

function familiesOf(cs: CastMember[]): Family[] {
  const by = new Map<string, Family>();
  for (const c of cs) {
    const k = c.family ?? c.model;
    const f = by.get(k);
    if (f) f.push(c);
    else by.set(k, [c]);
  }
  return [...by.values()];
}

function CastButton({ family, shown, picked, onPick }: { family: Family; shown: CastMember; picked: boolean; onPick: () => void }) {
  const name = family[0].family_name ?? family[0].name;
  const more = family.length > 1 ? `${family.length} outfits` : undefined;
  return (
    <UnstyledButton
      onClick={onPick}
      aria-pressed={picked}
      aria-label={more ? `${name}, ${more}` : name}
      title={shown.about || shown.name}
      p={6}
      style={{
        borderRadius: 10,
        border: `2px solid ${picked ? "var(--vox-gate)" : "var(--vox-veil)"}`,
        background: picked ? "#fff" : "rgba(251,252,254,0.6)",
        textAlign: "center",
      }}
    >
      <Image src={`/assets/${shown.portrait}`} alt="" loading="lazy" radius="sm" />
      <Text fz="xs" fw={700} mt={4} lineClamp={1}>
        {name}
      </Text>
      {more && (
        <Text fz={10} c="moon.5" lineClamp={1}>
          {more}
        </Text>
      )}
    </UnstyledButton>
  );
}
