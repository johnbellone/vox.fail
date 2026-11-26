// Feats: what authority is for. Each happens where you last stood in a world.

import { Alert, Anchor, Badge, Button, Card, Group, NumberInput, Select, Stack, Text, TextInput, Title } from "@mantine/core";
import { useState, type ReactNode } from "react";
import { messageOf, num, post, when } from "../../api/client";
import type { Feats } from "../../api/types";

function Feat({
  title,
  cost,
  have,
  about,
  children,
  run,
  ready = true,
}: {
  title: string;
  cost: number;
  have: number;
  about: ReactNode;
  children?: ReactNode;
  run?: () => Promise<string>;
  ready?: boolean;
}) {
  const [busy, setBusy] = useState(false);
  const [said, setSaid] = useState<{ ok: boolean; text: string }>();
  return (
    <Card withBorder radius="md" bg="rgba(255,255,255,0.6)">
      <form
        onSubmit={async (e) => {
          e.preventDefault();
          if (!run) return;
          setBusy(true);
          setSaid(undefined);
          try {
            setSaid({ ok: true, text: await run() });
          } catch (err) {
            setSaid({ ok: false, text: messageOf(err) });
          } finally {
            setBusy(false);
          }
        }}
      >
        <Stack gap="sm">
          <Group justify="space-between" wrap="nowrap">
            <Title order={3} fz="md">
              {title}
            </Title>
            <Badge variant="light" tt="none">
              {cost} authority
            </Badge>
          </Group>
          <Text fz="sm" c="moon.5">
            {about}
          </Text>
          {children}
          {run && (
            <Button type="submit" loading={busy} disabled={!ready}>
              {title}
            </Button>
          )}
          {run && have < cost && (
            <Text fz="sm" c="moon.5">
              You have {have}: keep playing to earn the rest.
            </Text>
          )}
          {said && (
            <Alert color={said.ok ? "gate" : "ember"} role={said.ok ? "status" : "alert"}>
              {said.text}
            </Alert>
          )}
        </Stack>
      </form>
    </Card>
  );
}

export function FeatsPanel({ feats: f, reload }: { feats: Feats; reload: () => Promise<void> }) {
  const c = f.costs;
  const places = f.places.map((p) => ({ value: p.world, label: `${p.world_name}, where you were on ${when(p.seen)}` }));
  const inPlot = f.places.filter((p) => p.in_your_plot).map((p) => ({ value: p.world, label: p.world_name }));
  const dests = f.worlds.map((w) => ({ value: w.id, label: w.name }));
  const issuable = f.owned_worlds.filter((w) => !w.currency).map((w) => ({ value: w.world, label: w.world_name }));

  const [plotWorld, setPlotWorld] = useState<string | null>(places[0]?.value ?? null);
  const [from, setFrom] = useState<string | null>(inPlot[0]?.value ?? null);
  const [to, setTo] = useState<string | null>(dests.find((d) => d.value !== inPlot[0]?.value)?.value ?? dests[0]?.value ?? null);
  const [curWorld, setCurWorld] = useState<string | null>(issuable[0]?.value ?? null);
  const [curName, setCurName] = useState("");
  const [curCode, setCurCode] = useState("");
  const [supply, setSupply] = useState<number | string>(10000);

  const after = async <T,>(p: Promise<T>) => {
    const r = await p;
    await reload();
    return r;
  };

  return (
    <Stack gap="md">
      <Feat
        title="Claim a plot"
        cost={c.plot}
        have={f.authority}
        ready={!!plotWorld}
        about="16 × 16 blocks around where you last stood, at least 24 blocks from where people arrive. Only you (and a group you choose) can build in it."
        run={async () => {
          const r = await after(post<{ area: { x0: number; x1: number; z0: number; z1: number } }>("/api/feats/plot", { world: plotWorld }));
          return `Claimed: x ${r.area.x0} to ${r.area.x1 - 1}, z ${r.area.z0} to ${r.area.z1 - 1}. The deed is in your inventory.`;
        }}
      >
        <Select label="Where" data={places} value={plotWorld} onChange={setPlotWorld} placeholder="Go into a world first" disabled={!places.length} />
      </Feat>

      <Feat
        title="Open a portal"
        cost={c.portal}
        have={f.authority}
        ready={!!from && !!to}
        about="A doorway, where you last stood in a plot of yours, to another world. Players arriving after it opens see it."
        run={async () => {
          const r = await after(post<{ at: number[] }>("/api/feats/portal", { world: from, to }));
          return `Opened. Its key is in your inventory (${r.at.join(", ")}).`;
        }}
      >
        <Select label="From" data={inPlot} value={from} onChange={setFrom} placeholder="Stand in a plot of yours first" disabled={!inPlot.length} />
        <Select label="To" data={dests} value={to} onChange={setTo} />
      </Feat>

      {issuable.length ? (
        <Feat
          title="Issue a currency"
          cost={c.currency}
          have={f.authority}
          ready={!!curWorld && !!curName && !!curCode}
          about="Give a world you own its own coin. You set the supply once; all of it is yours to give out."
          run={async () => {
            const r = await after(
              post<{ name: string; code: string; supply: number }>("/api/feats/currency", { world: curWorld, name: curName, code: curCode, supply: Number(supply) }),
            );
            return `${r.name} (${r.code}) issued: all ${num(r.supply)} are yours.`;
          }}
        >
          <Select label="For" data={issuable} value={curWorld} onChange={setCurWorld} />
          <TextInput label="Name" placeholder="Acorns" maxLength={24} value={curName} onChange={(e) => setCurName(e.currentTarget.value)} required />
          <TextInput label="Code (3 to 5 letters)" placeholder="ACRN" maxLength={5} value={curCode} onChange={(e) => setCurCode(e.currentTarget.value.toUpperCase())} required />
          <NumberInput label="Supply" min={1} max={1_000_000_000} value={supply} onChange={setSupply} allowDecimal={false} thousandSeparator="," required />
        </Feat>
      ) : (
        <Feat
          title="Issue a currency"
          cost={c.currency}
          have={f.authority}
          about={
            f.owned_worlds.length
              ? `Your worlds have their coins: ${f.owned_worlds.map((w) => `${w.currency} for ${w.world_name}`).join(", ")}.`
              : "A world's owner can give it its own coin, which anyone can then send and receive. You don't own a world yet."
          }
        />
      )}

      <Feat
        title="Start a raid"
        cost={c.raid}
        have={f.authority}
        about={
          <>
            Lead your party into another world. Raids start from <Anchor href="/friends" fz="sm">Friends and raids</Anchor>.
          </>
        }
      />
    </Stack>
  );
}
