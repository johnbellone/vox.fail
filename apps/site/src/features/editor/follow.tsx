// The shared pieces of an advanced form that sends one edit: following the edit it sent (its
// sending, refusal and done), taking the seq the world now has after its own Apply, and the notice
// for a refusal. The portal inspector, the spawn and objective forms and the settings form all use
// them, so each adopts the row's new seq after its own edit and never after someone else's.

import { Alert, Button, Stack, Text } from "@mantine/core";
import { useEffect, useRef, useState } from "react";
import type { Command } from "../../game/editor";
import { type Edits, refusalLine, retryCommand } from "./state";

export type Submit = (make: (ref: number) => Command) => number;

/** One panel's edit, followed by its ref: the open edit, its refusal, and whether it is sending.
 *  A done edit lets go (the panel is free again); a refusal stays with its text. */
export function useEditFollow(edits: Edits, submit: Submit, forget: (ref: number) => void) {
  const [pending, setPending] = useState<number>();
  const [problem, setProblem] = useState<string>();
  // counts the edits this panel saw go through (the client's `done`), so the panel can take the
  // seq the world now has (see useAdoptSaved)
  const [doneTick, setDoneTick] = useState(0);
  useEffect(() => {
    if (pending !== undefined && !edits.has(pending)) {
      setPending(undefined);
      setDoneTick((t) => t + 1);
    }
  }, [edits, pending]);

  const start = (make: (ref: number) => Command) => {
    if (pending !== undefined) forget(pending);
    setProblem(undefined);
    setPending(submit(make));
  };
  // lets go of the open edit and any problem, after a reload or before a new form
  const clear = () => {
    if (pending !== undefined) forget(pending);
    setPending(undefined);
    setProblem(undefined);
  };
  const mine = pending === undefined ? undefined : edits.get(pending);
  return { pending, problem, setProblem, busy: mine?.status === "sent", doneTick, start, clear };
}

/** After this panel's own edit goes through, the form expects the seq the row now has, so a second
 *  Apply is not refused as Changed. Waits for the row to show a seq other than the one the form
 *  holds (the `done` may arrive before the row does), then calls `adopt` with the row. A change by
 *  someone else, with no edit of ours settled, is not taken. */
export function useAdoptSaved<R extends { seq: number }>(
  row: R | undefined,
  seq: number,
  doneTick: number,
  adopt: (row: R) => void,
) {
  const seenDone = useRef(0);
  const waiting = useRef(false);
  useEffect(() => {
    if (doneTick > seenDone.current) {
      seenDone.current = doneTick;
      waiting.current = true;
    }
    if (!waiting.current || !row || row.seq === seq) return;
    waiting.current = false;
    adopt(row);
  }, [row, seq, doneTick]);
}

/** The refusal of a panel's open edit: its line, a Reload after a Changed refusal, and a retry
 *  after one the client calls Unavailable (the same ref, so nothing is charged twice). */
export function EditNotice({
  edits,
  pending,
  retry,
  reload,
}: {
  edits: Edits;
  pending: number | undefined;
  retry: (ref: number) => void;
  reload: () => void;
}) {
  const mine = pending === undefined ? undefined : edits.get(pending);
  if (pending === undefined || mine?.status !== "refused") return null;
  return (
    <Alert color="ember" role="alert">
      <Stack gap="xs">
        <Text fz="sm">{refusalLine(mine)}</Text>
        {mine.why === "Changed" ? (
          <Button size="xs" variant="default" onClick={reload} w="fit-content">
            Reload
          </Button>
        ) : (
          retryCommand(edits, pending) !== undefined && (
            <Button size="xs" variant="default" onClick={() => retry(pending)} w="fit-content">
              Try again
            </Button>
          )
        )}
      </Stack>
    </Alert>
  );
}
