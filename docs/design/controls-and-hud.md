# Design: block-game controls, HUD and menus

Status: **phases 1 to 4 built** (creative) · 2026-11-26 · health and survival inventories are not

Related: [world-scaling.md](world-scaling.md) (server authority, browser and desktop
budgets), [world-graph-and-art.md](world-graph-and-art.md) (art in the project's style).

## Summary

Vox plays like a block-building game: aim at a block, break it, place what's in your hand,
pick from a hotbar of nine, open an inventory, and watch your health. Every action works on
keyboard and mouse **and on a gamepad out of the box**, and every hint on screen shows the
button for **the device you last touched**, drawn for its brand (Xbox, PlayStation, Nintendo,
Steam). Every menu can be driven with a stick or d-pad: focus moves between controls, A (or
the brand's equivalent) chooses, B goes back.

## Where the code is today

- `world/src/client.rs` has a nine-slot block hotbar (`hotbar`, `slot`) shown as a line of
  text in the HUD, with number keys, the wheel and the bumpers to change slot.
- `controls` reads the keyboard, mouse and gamepads directly: South jumps, North toggles
  flying, the triggers break and place, the left stick click runs.
- The camera is Diablo-style (click to walk) or behind the shoulder (`V`); there is no
  first-person view and no pointer lock in the browser.
- Building is unlimited: there is no block inventory, no health and no damage. The
  directory's inventory (coins, deeds, wardrobe) is a separate thing on the web pages.
- There are no in-game menus; settings come from the command line or the control page.

## Goals

- Familiar block-game controls: first-person aim with a crosshair, break and place, hotbar,
  inventory, health.
- Gamepad support with no setup, on desktop and in the browser.
- Button hints that match the controller in your hands, and switch the moment you change
  device.
- Every screen navigable with a controller alone; mouse and keyboard work there too.
- Rebindable controls, saved per device.

## Non-goals (for now)

- Crafting, hunger and combat with creatures. The design leaves room for them.
- Touch controls on phones.

## Input: actions, not keys

A small action layer sits between devices and the game, in `vox-world` (a new `input`
module, later its own folder per [repo-structure.md](repo-structure.md)). Game code asks
"is Jump pressed?", never "is Space pressed?".

| Action | Keyboard and mouse | Gamepad (by position) |
| --- | --- | --- |
| Move | W A S D | Left stick |
| Look | Mouse | Right stick |
| Jump / fly up | Space | South |
| Sneak / fly down | Shift | Right stick click |
| Sprint | Ctrl, or double-tap W | Left stick click |
| Break | Left click (hold) | Right trigger (hold) |
| Place / use | Right click | Left trigger |
| Pick the block you aim at | Middle click | West |
| Hotbar next / previous | Wheel | Right / left bumper |
| Hotbar 1 to 9 | 1 to 9 | (bumpers, or the quick wheel) |
| Quick wheel (hold, point, let go) | Tab + mouse | D-pad left + right stick |
| Inventory | E | North |
| Menu (pause) | Esc | Start |
| Chat | T | Select (back) |
| Change view (first person, behind, Diablo) | F5 or V | D-pad up |
| Fly on / off | Double-tap Space | D-pad down |

- **Bindings are data**: a table per device kind, saved in the player's settings, edited in the
  Controls menu. Defaults above.
- **Analog done properly**: radial dead zone (0.15 by default), response curve for look,
  separate X and Y sensitivity, invert Y, and look acceleration on sticks so small corrections
  stay precise.
- **The last device wins**: any input from a device makes it the *active device*; hints,
  cursor visibility and menu focus follow it. Touching the mouse shows the cursor and hides
  the focus ring; touching a stick hides the cursor and shows focus.
- **First person**: becomes the default view. Desktop captures the cursor; in the browser the
  first click requests pointer lock (browsers need a click for it), and Esc releases it and
  opens the menu. The Diablo view stays available for players who like it.
- **Aim**: a ray from the camera centre (already in `vox-core::ray`), with the block outline
  that exists today. Gamepads get a slightly larger aim cone for small targets, no
  snapping.

## Controller detection and button hints

| Brand | How it's recognised | South / East / West / North shown as |
| --- | --- | --- |
| Xbox (and the default for unknown pads) | USB vendor 045e | A / B / X / Y |
| PlayStation | vendor 054c | Cross / Circle / Square / Triangle (shapes) |
| Nintendo | vendor 057e | B / A / Y / X (same positions, Nintendo's letters) |
| Steam Deck and Steam Controller | vendor 28de | A / B / X / Y with Steam's glyphs |
| Keyboard and mouse | | key caps and mouse buttons |

- Native builds read vendor and product IDs from Bevy's `Gamepad`. In the browser the Gamepad
  API's `id` string carries them (usually `"... (Vendor: 054c Product: 0ce6)"`); parse it, and
  fall back to the name, then to Xbox.
- Nintendo pads swap the *labels*, not the positions: Confirm stays South. A setting allows
  the Nintendo convention (Confirm on East) for players who prefer it.
- **Glyphs are pixel art in the game's style**: 12 × 12 or 16 × 16 sprites drawn the same way as
  the inventory glyphs on the site, one sheet per brand. Our own drawings, not copied logos;
  brand marks themselves (the PlayStation shapes are generic shapes) are not used.
- A `Hint` component in the UI (`Hint(Action::Jump)`) draws the active device's glyph and the
  label; it updates by itself when the device changes. The same glyph sheets are exported for
  the web pages.

## HUD

Drawn in `bevy_ui` in the pixel style (the Press Start 2P font, the game's palette):

- **Crosshair** in the centre (exists; becomes a sprite).
- **Hotbar** at the bottom centre: nine slots with block icons rendered from the block tiles,
  the selected slot framed, the item's name shown for two seconds after a change, counts in
  survival worlds.
- **Quick wheel**: while its button is held, the hotbar's nine blocks sit round a circle in
  the middle of the screen; the right stick (or the mouse) points, letting go takes that
  block. It selects hotbar slots, so it and the keyboard's number keys and wheel stay in
  step: the wheel is how a controller reaches any of the nine at once.
- **Health** above the hotbar on the left: ten hearts, half-heart steps. Flashes and shakes a
  little on damage; regenerates slowly when not hurt recently.
- **Breath** above it on the right when under water.
- **Hint bar** at the bottom edge when useful (first minutes, in menus, near portals):
  "[glyph] Break · [glyph] Place · [glyph] Inventory".
- **Notices** (joins, "the way is closed") top centre; the FPS counter stays top right.
- Everything scales with a UI scale setting and stays inside a safe area for TVs.

## Inventory and health: who decides

A modified client must not be able to give itself blocks or health, so **the server is
authoritative**, and the client predicts, as it already does for edits:

- **World modes** (a world setting): `creative` (today's unlimited building; no damage) and
  `survival` (blocks come from breaking them; damage on). **New worlds default to creative**
  (decided 2026-11-25); survival is opted into per world. In creative the inventory screen is a
  palette of every block to drag onto the hotbar, and health is hidden.
- **Block inventory** belongs to the player *in that world*: 9 hotbar slots + 27 backpack slots,
  stacks of up to 64. Breaking a block adds it; placing removes one. The server applies both
  with the edit itself, so they cannot drift apart. New messages: `ToClient::Inventory` (full on
  joining, then changes) and `ToServer::MoveItem { from, to, count }` for rearranging.
- **Health** is a number of half-hearts on the server. Damage comes from falls, drowning, fire
  and lava at first, computed on the server from the movement it receives, which needs the
  server-side movement checks already listed under *Known gaps* in `AGENTS.md`. At zero you
  respawn at the world's spawn, and in survival drop your backpack where you fell.
- **The directory's inventory** (coins, deeds, keys, trophies, wardrobe) stays account-wide and
  shows as a second tab in the in-game inventory, read-only, with a link to the site.
- Inventories are saved with the world (the zone format, see the world-format discussion),
  so snapshots include them.

## Menus with controller navigation

Built in `bevy_ui`, using Bevy's focus and **directional navigation** (`bevy_input_focus`): each
screen is a grid of focusable controls; the d-pad or left stick moves focus to the nearest
control in that direction, South confirms, East goes back, the bumpers switch tabs, and the
triggers page through long lists. The mouse and keyboard (Tab, arrows, Enter, Esc) work on the
same screens.

- **Pause menu**: Resume, Inventory, Controls, Settings, Leave world.
- **Inventory**: hotbar and backpack grid with the cursor moving between slots; South picks up a
  stack, West splits it, North moves it to the hotbar; a "world" tab and an "account" tab.
- **Controls**: the binding table for the active device; select an action, press the new
  button; reset to defaults. Sensitivity, invert Y, dead zone, aim assist cone.
- **Settings**: view distance (within the browser and desktop budgets), UI scale, FPS counter,
  hints on or off, Nintendo confirm button.
- **Focus is always visible**: a two-pixel frame in the gate blue, plus a sound. Focus is
  remembered per screen, and the first focus goes to the most likely choice (Resume).
- **Accessibility**: every control has a text label read by the accessibility tree (Bevy has
  AccessKit), and nothing relies on colour alone.

## Crate and module layout

```
crates/vox-world/src/
  input/
    mod.rs          Action, the action state resource, the plugin
    bindings.rs     default tables, loading and saving, rebinding
    devices.rs      active device, controller brand detection
    gamepad.rs      dead zones, curves, look acceleration
  hud/
    mod.rs          hotbar, health, breath, notices, hint bar
    glyphs.rs       glyph sheets per brand, the Hint component
  menu/
    mod.rs          screens and the navigation plugin
    pause.rs, inventory.rs, controls.rs, settings.rs
  player/           (from the client.rs split) movement, aim, break and place
crates/vox-core/src/
  inventory.rs      stacks, slots, the rules both sides share
  health.rs         damage rules (falls, drowning, fire)
```

The server applies `vox-core`'s inventory and health rules; the client uses the same code to
predict. Glyph sheets live in `assets/ui/glyphs/<brand>.png`.

## Testing

- Unit tests for the action layer (bindings, dead zones, device switching) and for inventory and
  health rules in `vox-core`.
- Server tests: breaking gives the block, placing takes it, placing without it is refused, fall
  damage, respawn.
- Navigation tests: every screen reachable and escapable with only directional input and
  South/East, run headless.
- Manual matrix before release: Xbox, PlayStation and Switch Pro controllers and a Steam Deck,
  on Linux, Windows and in Chrome and Firefox.

## Phases

1. Action layer and device detection; existing controls moved onto it; glyph sheets and the
   hint bar; first-person view with pointer lock.
2. HUD in `bevy_ui`: crosshair, hotbar with block icons, notices.
3. Menus with directional navigation: pause, controls (rebinding), settings.
4. The inventory screen as a creative block palette (the default mode); then server-side
   survival inventories for worlds that opt in.
5. Health and damage for survival worlds, after server-side movement checks.

## Open questions

- Which damage sources come first, and is there a penalty for dying beyond respawning?
- Does the account inventory (coins, wardrobe) need actions in game, or only viewing?
