// Tests for choosing a block from the palette: the command the page sends.

import { describe, expect, test } from "bun:test";
import { chooseBlock } from "./BlockPalette";

describe("chooseBlock", () => {
  test("a palette name becomes a block command with that name", () => {
    expect(chooseBlock("glass")).toEqual({ type: "block", name: "glass" });
  });

  test("the command has no other fields, so it matches the bridge's Command::Block", () => {
    expect(Object.keys(chooseBlock("stone")).sort()).toEqual(["name", "type"]);
  });
});
