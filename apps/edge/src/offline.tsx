// The page vox.fail shows while its server is offline (src/worker.ts serves it): what Vox is and
// what the worlds look like, and that it's away for now. It asks the directory for nothing, since
// there is no directory to ask.

import { Button, Card, Container, Image, Paper, SimpleGrid, Stack, Text, Title } from "@mantine/core";
import { mount } from "../../site/src/components/Shell";
import { Pitch } from "../../site/src/features/home/Pitch";
import { backTo } from "./back";

// build.ts copies docs/screenshots here; the captions are the README's, which the game drew
const SHOTS: { file: string; caption: string }[] = [
  { file: "void.png", caption: "Through the Void's doorway, the yard on the other side" },
  { file: "sea.png", caption: "The pier's fires over the Dark Sea's swell" },
  { file: "islands.png", caption: "A doorway among the floating islands" },
  { file: "yard.png", caption: "A character at home in the yard" },
  { file: "orbit.png", caption: "The planet from the orbital station's truss, 250 km up" },
];

function OfflinePage() {
  return (
    <Container size="lg" component="main" py={{ base: "xl", sm: 80 }}>
      <SimpleGrid cols={{ base: 1, md: 2 }} spacing={{ base: "xl", md: 64 }} style={{ alignItems: "center" }}>
        <Pitch />
        <Paper shadow="sm" p="lg" role="status">
          <Stack gap="sm">
            <Title order={2} fz="lg">
              Vox is offline right now
            </Title>
            <Text c="moon.5">
              The worlds are closed for the moment, so signing in and playing will have to wait. Everything built so
              far is kept. Come back soon.
            </Text>
            <Button variant="light" onClick={() => location.assign(backTo(location.search, location.origin))}>
              Try again
            </Button>
          </Stack>
        </Paper>
      </SimpleGrid>
      <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="lg" mt={{ base: "xl", sm: 80 }}>
        {SHOTS.map((s) => (
          <Card key={s.file} shadow="sm" padding={0}>
            <Image src={`/_offline/shots/${s.file}`} alt={s.caption} loading="lazy" />
            <Text size="sm" c="moon.5" p="sm">
              {s.caption}
            </Text>
          </Card>
        ))}
      </SimpleGrid>
    </Container>
  );
}

mount(<OfflinePage />);
