// Signed in: who you are, the way into the Void, and the desktop app.

import { Anchor, Avatar, Button, CopyButton, Divider, Group, Stack, Text, Title } from "@mantine/core";
import { useEffect, useState } from "react";
import { api, findCast, messageOf, post } from "../../api/client";
import { useApi } from "../../api/hooks";
import type { Catalogue, Me, Token, Wallet, Worlds } from "../../api/types";
import { SITES } from "./SignIn";

export function SignedIn({ me, onSignOut }: { me: Me; onSignOut: () => void }) {
  const cat = useApi<Catalogue>("/api/catalogue");
  const worlds = useApi<Worlds>("/api/worlds");
  const [authority, setAuthority] = useState<number>();
  useEffect(() => {
    api<Wallet>("/api/wallet")
      .then((w) => setAuthority(w.balances[0]?.amount))
      .catch(() => {});
  }, []);
  const c = findCast(cat.data, me.character?.model);
  const offered = me.sign_in_with ?? [];
  const linked = me.linked ?? [];
  const downloads = worlds.data?.downloads ?? [];

  return (
    <Stack gap="lg">
      <Group gap="md" wrap="nowrap">
        <Avatar src={c ? `/assets/${c.portrait}` : undefined} size={72} radius="md" bg="moon.1" alt="" />
        <div>
          <Title order={2} fz="xl">
            {me.name}
          </Title>
          {c && <Text c="moon.5">as {c.name}</Text>}
        </div>
      </Group>

      <Stack gap="xs">
        <Button component="a" href="/play" size="lg">
          Enter the Void
        </Button>
        <Button component="a" href="/create" variant="default">
          Change your character
        </Button>
      </Stack>

      <Group justify="center" gap="xl">
        <Anchor href="/inventory">{authority !== undefined ? `${authority} authority, inventory` : "Inventory"}</Anchor>
        <Anchor href="/friends">Friends and raids</Anchor>
      </Group>

      <Divider />

      <Stack gap="xs">
        <Title order={3} fz="md">
          Play on your desktop
        </Title>
        <Text c="moon.5" fz="sm">
          {downloads.length
            ? "The desktop app looks better and runs faster. Download it, then paste your launch code into it."
            : "Running the desktop app from source? Copy your launch code and start it with: vox-client --token <code>"}
        </Text>
        {downloads.length > 0 && (
          <Group gap="xs">
            {downloads.map((d) => (
              <Button key={d.url} component="a" href={d.url} variant="default" size="xs">
                Download for {d.label}
              </Button>
            ))}
          </Group>
        )}
        <LaunchCode />
      </Stack>

      <Group gap="md" justify="center" c="moon.5" fz="sm">
        {linked.length > 0 && <span>You sign in with {linked.map((s) => SITES[s] ?? s).join(" and ")}.</span>}
        {offered
          .filter((s) => !linked.includes(s))
          .map((s) => (
            <Anchor key={s} href={`/auth/${s}`} fz="sm">
              Link your {SITES[s] ?? s} account
            </Anchor>
          ))}
      </Group>

      <Button
        variant="subtle"
        color="moon"
        onClick={async () => {
          await post("/api/signout");
          onSignOut();
        }}
      >
        Sign out
      </Button>
    </Stack>
  );
}

/** Fetch a launch code (a sign-in token for the desktop app) and copy it. */
function LaunchCode() {
  const [code, setCode] = useState<string>();
  const [note, setNote] = useState<string>();
  if (code) {
    return (
      <Group gap="sm">
        <CopyButton value={code}>
          {({ copied, copy }) => (
            <Button variant="default" size="xs" onClick={copy}>
              {copied ? "Copied" : "Copy launch code"}
            </Button>
          )}
        </CopyButton>
        <Text c="moon.5" fz="sm">
          It works for a day.
        </Text>
      </Group>
    );
  }
  return (
    <Group gap="sm">
      <Button
        variant="default"
        size="xs"
        onClick={async () => {
          try {
            const t = await post<Token>("/api/token");
            await navigator.clipboard.writeText(t.token).then(
              () => setNote("Copied. It works for a day."),
              () => setNote(undefined),
            );
            setCode(t.token);
          } catch (err) {
            setNote(messageOf(err));
          }
        }}
      >
        Copy launch code
      </Button>
      {note && (
        <Text c="moon.5" fz="sm">
          {note}
        </Text>
      )}
    </Group>
  );
}
