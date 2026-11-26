import { Title } from "@mantine/core";

/** VOX, in blocks. `small` links home and sits in the corner of the inner pages. */
export function Wordmark({ small = false }: { small?: boolean }) {
  if (small) {
    return (
      <Title order={2} className="vox-wordmark" fz={18}>
        <a href="/" aria-label="Vox, home">
          VOX
        </a>
      </Title>
    );
  }
  return (
    <Title order={1} className="vox-wordmark" fz={{ base: 48, sm: 72 }}>
      VOX
    </Title>
  );
}
