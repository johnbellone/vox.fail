// The inner pages' frame: the wordmark and the row of places across the top, then the page.

import { Anchor, Container, Group, Stack, Text, Title } from "@mantine/core";
import type { ReactNode } from "react";
import { Wordmark } from "./Wordmark";

const PLACES: [string, string][] = [
  ["/", "Home"],
  ["/create", "Character"],
  ["/inventory", "Inventory"],
  ["/friends", "Friends and raids"],
];

export function Nav() {
  const here = typeof location === "undefined" ? "" : location.pathname;
  return (
    <Group component="nav" aria-label="Vox" gap="lg">
      {PLACES.map(([href, label]) => (
        <Anchor
          key={href}
          href={href}
          fw={700}
          c={here === href ? "moon.9" : "moon.5"}
          underline="never"
          aria-current={here === href ? "page" : undefined}
          style={{ borderBottom: `2px solid ${here === href ? "var(--vox-gate)" : "transparent"}`, paddingBlock: 6 }}
        >
          {label}
        </Anchor>
      ))}
    </Group>
  );
}

export function Page({
  title,
  intro,
  children,
  size = "xl",
}: {
  title?: string;
  intro?: ReactNode;
  children: ReactNode;
  size?: string;
}) {
  return (
    <Container size={size} py="lg" component="main">
      <Group justify="space-between" mb="xl" wrap="wrap" gap="md">
        <Wordmark small />
        <Nav />
      </Group>
      <Stack gap="lg">
        {title && <Title order={1}>{title}</Title>}
        {intro && (
          <Text c="moon.5" maw={760}>
            {intro}
          </Text>
        )}
        {children}
      </Stack>
    </Container>
  );
}
