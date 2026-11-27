// The note about when a name may next change: none when it may already, the date when it may not.

import { expect, test } from "bun:test";
import { renameNote } from "./NameChange";

test("no note when the name may change now", () => {
  expect(renameNote(null, 1_800_000_000)).toBeUndefined();
  expect(renameNote(undefined, 1_800_000_000)).toBeUndefined();
  expect(renameNote(1_700_000_000, 1_800_000_000)).toBeUndefined();
  expect(renameNote(1_800_000_000, 1_800_000_000)).toBeUndefined();
});

test("a note with the date when the name may change again", () => {
  const note = renameNote(1_900_000_000, 1_800_000_000);
  expect(note).toStartWith("You can change your name again on ");
  expect(note).toEndWith("Changing capitals only is fine any time.");
});
