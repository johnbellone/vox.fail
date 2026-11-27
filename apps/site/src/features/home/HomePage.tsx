// The start: what Vox is, and the door: sign in, or go in.

import { Container, Loader, Paper, SimpleGrid } from "@mantine/core";
import { useEffect } from "react";
import { useApi } from "../../api/hooks";
import type { Me } from "../../api/types";
import { LiveWorld } from "../../components/LiveWorld";
import { Problem } from "../../components/Problem";
import { Pitch } from "./Pitch";
import { SignIn } from "./SignIn";
import { SignedIn } from "./SignedIn";

export function HomePage() {
  const me = useApi<Me>("/api/me");
  // signed in but no character yet: make one first
  useEffect(() => {
    if (me.data?.name && !me.data.character) location.href = "/create";
  }, [me.data]);

  return (
    <>
    <LiveWorld />
    <Container size="lg" component="main" py={{ base: "xl", sm: 80 }}>
      <SimpleGrid cols={{ base: 1, md: 2 }} spacing={{ base: "xl", md: 64 }} style={{ alignItems: "center" }}>
        <Pitch />
        <Paper shadow="sm" aria-live="polite">
          {me.loading && !me.data ? (
            <Loader mx="auto" display="block" />
          ) : me.error ? (
            <Problem error={me.error} />
          ) : me.data?.name ? (
            <SignedIn me={me.data} onSignOut={() => void me.reload()} />
          ) : (
            <SignIn offered={me.data?.sign_in_with ?? []} onDone={() => void me.reload()} />
          )}
        </Paper>
      </SimpleGrid>
    </Container>
    </>
  );
}
