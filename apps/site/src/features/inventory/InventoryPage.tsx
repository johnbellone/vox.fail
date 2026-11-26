// The inventory explorer: everything you hold (coins, deeds, keys, banners, trophies, the
// wardrobe), your authority, the feats it buys, and the ledger.

import {
  Badge,
  Grid,
  Group,
  Loader,
  NavLink,
  Paper,
  Progress,
  Select,
  SimpleGrid,
  Stack,
  Text,
  TextInput,
  Title,
  UnstyledButton,
} from "@mantine/core";
import { useMemo, useState } from "react";
import { api, num, when } from "../../api/client";
import { useLoad, useSignedIn } from "../../api/hooks";
import type { Feats, Inventory, Item, People, Wallet } from "../../api/types";
import { Glyph } from "../../components/Glyph";
import { Page } from "../../components/Page";
import { Problem } from "../../components/Problem";
import { FeatsPanel } from "./FeatsPanel";
import { ItemDetail } from "./ItemDetail";
import { KINDS, art, inKind, shown, sub, type Filter, type Sort } from "./model";

interface Data {
  inv: Inventory;
  wallet: Wallet;
  feats: Feats;
  people: People;
}

export function InventoryPage() {
  const me = useSignedIn();
  const load = useLoad<Data>(async () => {
    const [inv, wallet, feats, people] = await Promise.all([
      api<Inventory>("/api/inventory"),
      api<Wallet>("/api/wallet"),
      api<Feats>("/api/feats"),
      api<People>("/api/people"),
    ]);
    return { inv, wallet, feats, people };
  });
  const [filter, setFilter] = useState<Filter>({ kind: "", world: "", q: "", sort: "new" });
  const [picked, setPicked] = useState<string>();

  const items = load.data?.inv.items ?? [];
  const worlds = useMemo(() => {
    const seen = new Map<string, string>();
    for (const i of items) if (i.world) seen.set(i.world, i.world_name ?? i.world);
    return [...seen].map(([value, label]) => ({ value, label }));
  }, [items]);
  const visible = useMemo(() => shown(items, filter), [items, filter]);

  if (load.error || me.error) return <Page title="Inventory"><Problem error={load.error ?? me.error} /></Page>;
  if (!load.data) return <Page title="Inventory"><Loader /></Page>;
  const { wallet, feats, people } = load.data;
  const set = (f: Partial<Filter>) => setFilter((old) => ({ ...old, ...f }));

  return (
    <Page title="Inventory">
      <Standing wallet={wallet} />

      <Grid gap="lg">
        <Grid.Col span={{ base: 12, md: 3 }}>
          <Stack gap="sm" component="aside" aria-label="Show">
            <Stack gap={2} role="group" aria-label="What to show">
              {KINDS.map(([k, label]) => {
                const n = items.filter((i) => inKind(i, k)).length;
                if (k && !n && k !== "deed") return null;
                return (
                  <NavLink
                    key={k || "all"}
                    label={label}
                    active={filter.kind === k}
                    onClick={() => set({ kind: k })}
                    rightSection={<Badge variant="light" color="moon" size="sm">{n}</Badge>}
                    aria-pressed={filter.kind === k}
                    style={{ borderRadius: 10 }}
                  />
                );
              })}
            </Stack>
            <Select label="World" data={[{ value: "", label: "Every world" }, ...worlds]} value={filter.world} onChange={(v) => set({ world: v ?? "" })} allowDeselect={false} />
            <TextInput label="Find" type="search" placeholder="By name" value={filter.q} onChange={(e) => set({ q: e.currentTarget.value })} />
            <Select
              label="Order"
              data={[
                { value: "new", label: "Newest first" },
                { value: "name", label: "By name" },
                { value: "kind", label: "By kind" },
              ]}
              value={filter.sort}
              onChange={(v) => set({ sort: (v ?? "new") as Sort })}
              allowDeselect={false}
            />
          </Stack>
        </Grid.Col>

        <Grid.Col span={{ base: 12, md: 5 }}>
          <section aria-label="What you hold">
            {visible.length ? (
              <SimpleGrid cols={{ base: 2, sm: 3 }} spacing="sm">
                {visible.map((i) => (
                  <ItemTile key={i.id} i={i} picked={picked === i.id} onPick={() => setPicked(i.id)} />
                ))}
              </SimpleGrid>
            ) : (
              <Text c="moon.5" ta="center" py="xl">
                {filter.kind === "deed" && !filter.q ? "No plots yet. Claim one under Feats, below." : "Nothing here matches."}
              </Text>
            )}
          </section>
        </Grid.Col>

        <Grid.Col span={{ base: 12, md: 4 }}>
          <Paper aria-live="polite">
            <ItemDetail
              item={items.find((i) => i.id === picked)}
              groups={people.crews.filter((c) => c.kind === "group")}
              reload={load.reload}
              onGone={() => setPicked(undefined)}
            />
          </Paper>
        </Grid.Col>
      </Grid>

      <SimpleGrid cols={{ base: 1, md: 2 }} spacing="lg">
        <Paper aria-labelledby="feats-h">
          <Title order={2} id="feats-h">
            Feats
          </Title>
          <Text c="moon.5" mb="md">
            What authority is for. Feats happen where you last stood in a world, so go there first.
          </Text>
          <FeatsPanel feats={feats} reload={load.reload} />
        </Paper>
        <Paper aria-labelledby="ledger-h">
          <Title order={2} id="ledger-h">
            Ledger
          </Title>
          <Text c="moon.5" mb="md">
            Every coin and every point of authority you've earned, spent, sent or been sent.
          </Text>
          <Ledger wallet={wallet} />
        </Paper>
      </SimpleGrid>
    </Page>
  );
}

function Standing({ wallet }: { wallet: Wallet }) {
  const auth = wallet.balances.find((b) => b.code === "AUTH");
  const e = wallet.earning;
  return (
    <Paper>
      <Group gap="xl" wrap="wrap">
        <div>
          <Text fz={44} fw={700} lh={1} c="gate.8">
            {num(auth?.amount ?? 0)}
          </Text>
          <Text c="moon.5">authority</Text>
        </div>
        <Stack gap={6} style={{ flex: 1, minWidth: 220 }}>
          <Text fz="sm">
            Earned today: {e.today} of {e.daily}
          </Text>
          <Progress value={Math.min(100, (e.today / Math.max(1, e.daily)) * 100)} aria-hidden />
          <Text fz="sm" c="moon.5">
            One for every {e.minutes_per} minutes in a world and every {e.edits_per} blocks you place.
          </Text>
        </Stack>
      </Group>
    </Paper>
  );
}

function ItemTile({ i, picked, onPick }: { i: Item; picked: boolean; onPick: () => void }) {
  return (
    <UnstyledButton
      onClick={onPick}
      aria-pressed={picked}
      p="sm"
      style={{
        borderRadius: 14,
        border: `2px solid ${picked ? "var(--vox-gate)" : "var(--vox-veil)"}`,
        background: picked ? "#fff" : "rgba(251,252,254,0.7)",
      }}
    >
      <Stack gap={6} align="center" ta="center">
        <Glyph kind={art(i)} authority={i.code === "AUTH"} size={44} />
        <Text fw={700} fz="sm" lineClamp={2}>
          {i.name}
        </Text>
        <Text fz="xs" c="moon.5" lineClamp={1}>
          {sub(i)}
        </Text>
      </Stack>
    </UnstyledButton>
  );
}

function Ledger({ wallet }: { wallet: Wallet }) {
  if (!wallet.ledger.length) return <Text c="moon.5">Nothing yet.</Text>;
  return (
    <Stack gap="xs" component="ul" p={0} m={0} style={{ listStyle: "none" }}>
      {wallet.ledger.slice(0, 25).map((l, n) => {
        const code = l.code === "AUTH" ? "authority" : l.code;
        const sign = l.amount > 0 ? "+" : "−";
        return (
          <Group key={n} component="li" justify="space-between" wrap="nowrap" align="flex-start">
            <div>
              <Text fz="sm">{l.memo}</Text>
              <Text fz="xs" c="moon.5">
                {when(l.at)}
                {l.with ? (l.amount > 0 ? `, from ${l.with}` : `, to ${l.with}`) : ""}
              </Text>
            </div>
            <Text fz="sm" fw={700} c={l.amount > 0 ? "gate.7" : "ember.6"} style={{ whiteSpace: "nowrap" }}>
              {sign}
              {num(Math.abs(l.amount))} {code}
            </Text>
          </Group>
        );
      })}
    </Stack>
  );
}
