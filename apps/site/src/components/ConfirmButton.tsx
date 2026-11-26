// A button that asks once before doing something that can't be undone: the first click turns
// it into the question, the second does it.

import { Button, type ButtonProps } from "@mantine/core";
import { useState, type ReactNode } from "react";

export function ConfirmButton({
  children,
  confirm,
  onConfirm,
  ...props
}: ButtonProps & { children: ReactNode; confirm: ReactNode; onConfirm: () => void | Promise<void> }) {
  const [asking, setAsking] = useState(false);
  const [busy, setBusy] = useState(false);
  return (
    <Button
      variant={asking ? "filled" : "subtle"}
      color={asking ? "ember" : "moon"}
      loading={busy}
      onBlur={() => setAsking(false)}
      onClick={async () => {
        if (!asking) return setAsking(true);
        setBusy(true);
        try {
          await onConfirm();
        } finally {
          setBusy(false);
          setAsking(false);
        }
      }}
      {...props}
    >
      {asking ? confirm : children}
    </Button>
  );
}
