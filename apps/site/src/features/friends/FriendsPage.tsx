// Friends, groups and raid parties.

import { Alert, Button, Group, Loader, Paper, SimpleGrid, Stack, Text, Title } from "@mantine/core";
import { api } from "../../api/client";
import { useLoad, useSignedIn } from "../../api/hooks";
import type { Catalogue, Feats, People } from "../../api/types";
import { ConfirmButton } from "../../components/ConfirmButton";
import { Page } from "../../components/Page";
import { Portrait } from "../../components/Portrait";
import { Problem } from "../../components/Problem";
import { useAct } from "./act";
import { CrewCard } from "./Crews";
import { NameForm } from "./NameForm";

interface Data {
  people: People;
  worlds: Feats["worlds"];
  cat: Catalogue;
}

export function FriendsPage() {
  const me = useSignedIn();
  const load = useLoad<Data>(async () => {
    const [people, feats, cat] = await Promise.all([api<People>("/api/people"), api<Feats>("/api/feats"), api<Catalogue>("/api/catalogue")]);
    return { people, worlds: feats.worlds, cat };
  });
  const { act, error, busy } = useAct(load.reload);

  const intro =
    "Friends can join your groups and parties. A group builds together: share a plot with it and everyone in it can build there. A raid party goes somewhere together: when two or more of you spend ten minutes in the world you raid, you've won.";

  if (load.error || me.error) return <Page title="Friends and raids"><Problem error={load.error ?? me.error} /></Page>;
  if (!load.data) return <Page title="Friends and raids"><Loader /></Page>;
  const { people: p, worlds, cat } = load.data;
  const groups = p.crews.filter((c) => c.kind === "group");
  const parties = p.crews.filter((c) => c.kind === "party");
  const ask = async (path: string, body: unknown) => (await act(path, body)) !== null;

  return (
    <Page title="Friends and raids" intro={intro}>
      {error && (
        <Alert color="ember" role="alert" withCloseButton={false}>
          {error}
        </Alert>
      )}

      {p.asking.length + p.invites.length > 0 && (
        <Paper aria-labelledby="inv-h">
          <Title order={2} id="inv-h" mb="sm">
            Waiting for you
          </Title>
          <Stack gap="xs">
            {p.asking.map((name) => (
              <Group key={`f-${name}`} justify="space-between">
                <Text>{name} wants to be your friend</Text>
                <Group gap="xs">
                  <Button size="xs" onClick={() => void act("/api/friends/answer", { name, accept: true })}>
                    Accept
                  </Button>
                  <Button size="xs" variant="subtle" color="moon" onClick={() => void act("/api/friends/answer", { name, accept: false })}>
                    Decline
                  </Button>
                </Group>
              </Group>
            ))}
            {p.invites.map((i) => (
              <Group key={`c-${i.crew}`} justify="space-between">
                <Text>
                  {i.by} invites you to the {i.kind === "party" ? "raid party" : "group"} {i.name}
                </Text>
                <Group gap="xs">
                  <Button size="xs" onClick={() => void act("/api/crews/answer", { crew: i.crew, accept: true })}>
                    Join
                  </Button>
                  <Button size="xs" variant="subtle" color="moon" onClick={() => void act("/api/crews/answer", { crew: i.crew, accept: false })}>
                    No thanks
                  </Button>
                </Group>
              </Group>
            ))}
          </Stack>
        </Paper>
      )}

      <SimpleGrid cols={{ base: 1, md: 3 }} spacing="lg">
        <Paper aria-labelledby="f-h">
          <Stack gap="md">
            <Title order={2} id="f-h">
              Friends
            </Title>
            <NameForm label="Add a friend by the name they go by" placeholder="Their name" button="Ask" busy={busy} onSubmit={(name) => ask("/api/friends", { name })} />
            <Stack gap="xs">
              {p.friends.map((f) => (
                <Group key={f.name} justify="space-between" wrap="nowrap">
                  <Group gap="sm" wrap="nowrap">
                    <Portrait cat={cat} model={f.model} size={36} />
                    <Text fw={700}>{f.name}</Text>
                  </Group>
                  <ConfirmButton size="xs" confirm={`Remove ${f.name}?`} onConfirm={() => void act("/api/friends/remove", { name: f.name })}>
                    Remove
                  </ConfirmButton>
                </Group>
              ))}
              {!p.friends.length && <Text c="moon.5">No friends here yet. Ask someone by name.</Text>}
            </Stack>
            {p.asked.length > 0 && (
              <Text fz="sm" c="moon.5">
                Waiting to hear from {p.asked.join(", ")}.
              </Text>
            )}
          </Stack>
        </Paper>

        <Paper aria-labelledby="g-h">
          <Stack gap="md">
            <div>
              <Title order={2} id="g-h">
                Groups
              </Title>
              <Text fz="sm" c="moon.5">
                Up to 24 friends who build together.
              </Text>
            </div>
            <NameForm label="Make a group" placeholder="A name for the group" button="Make" busy={busy} onSubmit={(name) => ask("/api/crews", { kind: "group", name })} />
            {groups.map((c) => (
              <CrewCard key={c.id} c={c} friends={p.friends} worlds={worlds} act={act} />
            ))}
            {!groups.length && <Text c="moon.5">You're not in a group.</Text>}
          </Stack>
        </Paper>

        <Paper aria-labelledby="p-h">
          <Stack gap="md">
            <div>
              <Title order={2} id="p-h">
                Raid parties
              </Title>
              <Text fz="sm" c="moon.5">
                Up to 8. The leader starts a raid on a world (10 authority); it lasts an hour. Everyone there when it's won
                gets a trophy and 15 authority (from up to two raids a day).
              </Text>
            </div>
            <NameForm label="Make a party" placeholder="A name for the party" button="Make" busy={busy} onSubmit={(name) => ask("/api/crews", { kind: "party", name })} />
            {parties.map((c) => (
              <CrewCard key={c.id} c={c} friends={p.friends} worlds={worlds} act={act} />
            ))}
            {!parties.length && <Text c="moon.5">You're not in a raid party.</Text>}
          </Stack>
        </Paper>
      </SimpleGrid>
    </Page>
  );
}
