import { Alert } from "@mantine/core";
import { messageOf } from "../api/client";

/** What went wrong, said plainly; nothing when nothing did. */
export function Problem({ error }: { error: unknown }) {
  if (!error) return null;
  return (
    <Alert color="ember" role="alert">
      {typeof error === "string" ? error : messageOf(error)}
    </Alert>
  );
}
