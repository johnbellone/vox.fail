// What a picked item is, and what you can do with it: send coins, share or give up a plot.

import { Alert, Button, Group, NumberInput, Select, Stack, Table, Text, TextInput, Title } from "@mantine/core";
import { useState } from "react";
import { messageOf, num, post, when } from "../../api/client";
import type { Crew, Item } from "../../api/types";
import { ConfirmButton } from "../../components/ConfirmButton";
import { Glyph } from "../../components/Glyph";
import { ABOUT, art } from "./model";

export function ItemDetail({ item: i, groups, reload, onGone }: { item: Item | undefined; groups: Crew[]; reload: () => Promise<void>; onGone: () => void }) {
  if (!i) {
    return <Text c="moon.5">Pick something to see what it is and what you can do with it.</Text>;
  }
  const rows: [string, string][] = [];
  if (i.kind === "coin") {
    rows.push(["You hold", `${num(i.amount ?? 0)} ${i.code}`]);
    if (i.world_name) rows.push(["Issued by", i.world_name]);
  } else {
    if (i.world_name) rows.push(["World", i.world_name]);
    if (i.acquired) rows.push(["Since", when(i.acquired)]);
    const a = i.detail.area;
    if (i.kind === "deed" && a) {
      rows.push(["Where", `x ${a.x0} to ${a.x1 - 1}, z ${a.z0} to ${a.z1 - 1}`]);
      rows.push(["Builders", i.detail.shared_with ? `you and ${i.detail.shared_with.name}` : "you"]);
    }
    if (i.kind === "key" && i.detail.to_name) rows.push(["Leads to", i.detail.to_name]);
    if (i.kind === "banner" && i.detail.raid_on) rows.push(["Raid on", i.detail.raid_on]);
    if (i.kind === "trophy") {
      if (i.detail.party) rows.push(["Party", i.detail.party]);
      if (i.detail.with !== undefined) rows.push(["Members", String(i.detail.with)]);
    }
    if (i.kind === "wear" && i.slot) rows.push(["Worn on", i.slot]);
  }
  const about = i.kind === "coin" ? i.detail.about : ABOUT[i.kind];

  return (
    <Stack gap="md">
      <Group gap="md" wrap="nowrap">
        <Glyph kind={art(i)} authority={i.code === "AUTH"} size={56} />
        <Title order={2} fz="lg">
          {i.name}
        </Title>
      </Group>
      {about && (
        <Text c="moon.5" fz="sm">
          {about}
        </Text>
      )}
      {rows.length > 0 && (
        <Table variant="vertical" withTableBorder={false} layout="fixed">
          <Table.Tbody>
            {rows.map(([k, v]) => (
              <Table.Tr key={k}>
                <Table.Th w={110}>{k}</Table.Th>
                <Table.Td>{v}</Table.Td>
              </Table.Tr>
            ))}
          </Table.Tbody>
        </Table>
      )}
      {i.kind === "coin" && i.code !== "AUTH" && <SendForm item={i} reload={reload} />}
      {i.kind === "deed" && <DeedActions item={i} groups={groups} reload={reload} onGone={onGone} />}
      {i.kind === "wear" && (
        <Button component="a" href="/create" variant="default">
          Change what you wear
        </Button>
      )}
    </Stack>
  );
}

function SendForm({ item: i, reload }: { item: Item; reload: () => Promise<void> }) {
  const [to, setTo] = useState("");
  const [amount, setAmount] = useState<number | string>("");
  const [memo, setMemo] = useState("");
  const [busy, setBusy] = useState(false);
  const [said, setSaid] = useState<{ ok: boolean; text: string }>();
  return (
    <form
      onSubmit={async (e) => {
        e.preventDefault();
        setBusy(true);
        setSaid(undefined);
        try {
          const r = await post<{ sent: number; code: string; to: string }>("/api/send", { to: to.trim(), code: i.code, amount: Number(amount), memo });
          await reload();
          setSaid({ ok: true, text: `Sent ${num(r.sent)} ${r.code} to ${r.to}.` });
          setAmount("");
          setMemo("");
        } catch (err) {
          setSaid({ ok: false, text: messageOf(err) });
        } finally {
          setBusy(false);
        }
      }}
    >
      <Stack gap="sm">
        <Title order={3} fz="md">
          Send {i.code}
        </Title>
        <TextInput label="To" placeholder="Their name" value={to} onChange={(e) => setTo(e.currentTarget.value)} required autoComplete="off" />
        <NumberInput label="How many" min={1} max={i.amount} value={amount} onChange={setAmount} required allowDecimal={false} />
        <TextInput label="Note (they'll see it)" maxLength={80} value={memo} onChange={(e) => setMemo(e.currentTarget.value)} />
        <Button type="submit" loading={busy}>
          Send
        </Button>
        {said && (
          <Alert color={said.ok ? "gate" : "ember"} role={said.ok ? "status" : "alert"}>
            {said.text}
          </Alert>
        )}
      </Stack>
    </form>
  );
}

function DeedActions({ item: i, groups, reload, onGone }: { item: Item; groups: Crew[]; reload: () => Promise<void>; onGone: () => void }) {
  const [share, setShare] = useState<string>(i.detail.shared_with ? String(i.detail.shared_with.id) : "");
  const [error, setError] = useState<string>();
  return (
    <Stack gap="sm">
      <Title order={3} fz="md">
        Who can build here
      </Title>
      <Group gap="xs" align="flex-end" wrap="nowrap">
        <Select
          label="Share with a group"
          data={[{ value: "", label: "Just you" }, ...groups.map((g) => ({ value: String(g.id), label: g.name }))]}
          value={share}
          onChange={(v) => setShare(v ?? "")}
          allowDeselect={false}
          style={{ flex: 1 }}
        />
        <Button
          onClick={async () => {
            setError(undefined);
            try {
              await post("/api/plots/share", { plot: i.ref, group: share ? Number(share) : null });
              await reload();
            } catch (err) {
              setError(messageOf(err));
            }
          }}
        >
          Share
        </Button>
      </Group>
      {!groups.length && (
        <Text fz="sm" c="moon.5">
          Make a group on Friends and raids to build here together.
        </Text>
      )}
      <div>
        <ConfirmButton
          confirm="Give it up for good (its portals close too)"
          onConfirm={async () => {
            try {
              await post("/api/plots/release", { plot: i.ref });
              onGone();
              await reload();
            } catch (err) {
              setError(messageOf(err));
            }
          }}
        >
          Give up this plot
        </ConfirmButton>
      </div>
      {error && (
        <Alert color="ember" role="alert">
          {error}
        </Alert>
      )}
    </Stack>
  );
}
