// Groups and raid parties: who's in them, inviting friends, raids, leaving.

import { Alert, Badge, Button, Card, Group, Progress, Select, Stack, Text, Title } from "@mantine/core";
import { useState } from "react";
import type { Crew, Friend } from "../../api/types";
import { ConfirmButton } from "../../components/ConfirmButton";

type Act = <T>(path: string, body: unknown) => Promise<T | null>;
const minutes = (s: number) => Math.max(0, Math.round(s / 60));

export function CrewCard({ c, friends, worlds, act }: { c: Crew; friends: Friend[]; worlds: { id: string; name: string }[]; act: Act }) {
  return (
    <Card withBorder radius="md" bg="rgba(255,255,255,0.6)">
      <Stack gap="sm">
        <Title order={3} fz="md">
          {c.name}
        </Title>
        <Group gap={6}>
          {c.members.map((m) => (
            <Badge key={m.name} variant={m.leads ? "filled" : "light"} color={m.leads ? "gate" : "moon"} tt="none">
              {m.leads && c.kind === "party" ? `${m.name}, leading` : m.name}
            </Badge>
          ))}
          {c.invited.map((n) => (
            <Badge key={n} variant="outline" color="moon" tt="none">
              {n}, invited
            </Badge>
          ))}
        </Group>
        {c.kind === "party" && <RaidBox c={c} worlds={worlds} act={act} />}
        {(c.kind === "group" || c.you_lead) && <Invite c={c} friends={friends} act={act} />}
        <div>
          <ConfirmButton
            size="xs"
            confirm={c.members.length === 1 ? `Leave and end ${c.name}?` : `Leave ${c.name}?`}
            onConfirm={() => void act("/api/crews/leave", { crew: c.id })}
          >
            Leave {c.name}
          </ConfirmButton>
        </div>
      </Stack>
    </Card>
  );
}

function Invite({ c, friends, act }: { c: Crew; friends: Friend[]; act: Act }) {
  const inside = new Set([...c.members.map((m) => m.name), ...c.invited]);
  const can = friends.filter((f) => !inside.has(f.name));
  const [pick, setPick] = useState<string | null>(null);
  if (!can.length) {
    return (
      <Text fz="sm" c="moon.5">
        {friends.length ? "All your friends are in it or invited." : "Make friends to invite them."}
      </Text>
    );
  }
  return (
    <Group gap="xs" wrap="nowrap">
      <Select
        aria-label={`Invite a friend to ${c.name}`}
        placeholder="A friend"
        data={can.map((f) => f.name)}
        value={pick}
        onChange={setPick}
        searchable
        style={{ flex: 1 }}
      />
      <Button variant="default" disabled={!pick} onClick={() => pick && void act("/api/crews/invite", { crew: c.id, name: pick })}>
        Invite
      </Button>
    </Group>
  );
}

function RaidBox({ c, worlds, act }: { c: Crew; worlds: { id: string; name: string }[]; act: Act }) {
  const r = c.raid;
  const [world, setWorld] = useState<string | null>(worlds[0]?.id ?? null);
  if (r && r.state === "on") {
    const left = minutes(r.ends - Date.now() / 1000);
    return (
      <Alert color="gate" title={`Raiding ${r.world_name}`}>
        <Stack gap="xs">
          <Text fz="sm">
            {r.together} of {r.needed} minutes together so far; {left} minutes left.
          </Text>
          <Progress value={Math.min(100, (r.together / r.needed) * 100)} />
          <div>
            <Button component="a" href={`/play?world=${encodeURIComponent(r.world_name)}`} size="xs">
              Go to {r.world_name}
            </Button>
          </div>
        </Stack>
      </Alert>
    );
  }
  return (
    <Stack gap="xs">
      {r && (
        <Text fz="sm" c="moon.5">
          {r.state === "won" ? `Won the raid on ${r.world_name}.` : `The raid on ${r.world_name} ran out of time.`}
        </Text>
      )}
      {c.you_lead ? (
        <Group gap="xs" wrap="nowrap">
          <Select aria-label="World to raid" data={worlds.map((w) => ({ value: w.id, label: w.name }))} value={world} onChange={setWorld} style={{ flex: 1 }} />
          <Button disabled={!world} onClick={() => world && void act("/api/feats/raid", { party: c.id, world })}>
            Start a raid (10 authority)
          </Button>
        </Group>
      ) : (
        <Text fz="sm" c="moon.5">
          The leader starts the next raid.
        </Text>
      )}
    </Stack>
  );
}
