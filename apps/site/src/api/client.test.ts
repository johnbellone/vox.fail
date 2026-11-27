// When the edge Worker says vox is offline, the site goes to its offline page and can come back.

import { expect, test } from "bun:test";
import { offlineAddress } from "./client";

test("only the Worker's offline answer leads to the offline page", () => {
  expect(offlineAddress("vox is offline right now; try again later", 503, "/play?world=void")).toBe(
    "/_offline/offline.html?back=%2Fplay%3Fworld%3Dvoid",
  );
  // the directory's own "busy" 503, and other errors, stay on the page
  expect(offlineAddress("busy: try again in a moment", 503, "/")).toBeUndefined();
  expect(offlineAddress("vox is offline right now", 500, "/")).toBeUndefined();
});
