# Vox: playing and running worlds

How to run a world, sign in from the web pages, and what authority, currencies, plots, portals,
friends and raids are. For how to play, see the [player help](../help/README.md). For a quick local game see the [README](../../README.md); for deploying
vox.fail see [deploy/README.md](../../deploy/README.md); the design documents are in
[docs/design](../design).

## Worlds: build together

A Vox world is a shared block space that stays: what people build is kept.

1. **Run a world:** `cargo run --release -p vox-server -- --listen 0.0.0.0:7780 --db
   world.db`. A new database starts as **the Void** (`--seed data/levels/yard.json` for the
   yard, or any level); after that it's whatever was built. Ctrl-C saves.
   `http://<server>:7780/` says who's in.
2. **Join:** `block_avatar --world ws://<server>:7780 --name Sam`.

**Voice:** `--voice-radius <blocks>` (default 32) sets how far nearby voice carries. The server
decides who hears each voice: players within that radius, and raid party members in the same
world at any distance.

## Play Vox from a web page

Players sign in on a web page, make their character there, and walk into the Void in the
browser, or download the desktop client.

1. **Run the directory** (accounts, characters, the pages):
   `cargo run --release -p vox-directory -- --listen 0.0.0.0:7700`. It makes a signing key
   the first time (`directory.key`) and writes its public half to `directory.key.pub`.
2. **Run the Void so it checks sign-ins:**
   `vox-server --listen 0.0.0.0:7780 --db void.db --directory-key directory.key.pub`
   (add `--guests` to let people in without signing in, as guests).
3. **Build the browser client and the pages:** `tools/dev/build_web.sh` (puts the client in
   `apps/site/public/pkg`; needs the wasm32 target and wasm-bindgen-cli, see the script), then
   `bun install && (cd apps/site && bun run build)`. The directory serves `apps/site/dist`.
4. Open `http://<server>:7700/`: **Sign in with Steam** or **Sign in with Discord** (when the
   directory has them, below), or **New account** with a name and password; pick who you are (38 characters, Blue first) and what
   you wear (a hat, something on your face, something in your hand, seen on a turntable),
   **Save character**, then **Enter the Void**. The browser needs WebGPU (current Chrome,
   Edge, Safari and Firefox have it).

**Desktop instead:** copy your launch code on the start page and run
`vox-client --token <code>` (`--world ws://host:7780/ws` for another server). The code
lasts a day. List downloads on the page with
`--download "Windows=https://…/vox-client.zip"`.

Worlds on the page come from `--world "The Void=:7780"` (repeatable; `:7780` is the page's own
host, that port; `/w/7780/ws` a path on it).

**Steam and Discord sign-in:** start the directory with `--steam` and `--public-url <its
address>` (Steam sends people back there; no key needed), and set `VOX_DISCORD_CLIENT_ID` and
`VOX_DISCORD_CLIENT_SECRET` from a Discord application whose OAuth2 redirect is
`<address>/auth/discord/return`. With `VOX_STEAM_KEY` (a Steam Web API key) new accounts take
their Steam name. A first sign-in makes an account named after you there (the nearest free
name); signed in already, **Link your Steam account** adds it to yours. Name and password
still work.

**Authority, coins, the inventory, friends and raids.** Authority is the currency every
world shares: 25 to start, then one for every 10 minutes in a world and every 50 blocks you
place (up to 60 a day), and 15 for each raid your party wins. It can't be sent: it's spent on
feats. **Claim a plot** (20): 16 × 16 blocks around where you last stood, which only you, or a
group you share it with, can build in. **Open a portal** (30): a doorway in your plot to another
world. **Issue a currency** (100): a world's owner gives it its own coin, with a fixed supply,
which anyone can then send to anyone. **Dungeons**: the Low Door, south of the Circle in the Void, leads into the Crypt. A doorway
whose url is `dungeon:<name>` leads to your raid party's own copy of that
dungeon (`data/levels/dungeons/<name>.json`): nobody else can come in, and it's gone an hour
later or once it has been empty for five minutes. A party opens at most three copies an hour,
and coming back to one that has gone (a fresh copy) counts as one. **Start a raid** (10): your
party's leader picks a world, or a dungeon (`dungeon:crypt`); in a world, when two or more of you
spend ten minutes there together within the hour, everyone there gets 15 authority and a
trophy; in a dungeon, when one of you reaches its objective (the Crypt's altar), every one of
the party who's in there with you does. The **Inventory** page holds coins, deeds, portal keys, banners,
trophies and the wardrobe (filter by kind and world, search, sort), the feats and the ledger
of every coin moved. **Friends and raids** has friend requests, groups (up to 24, who build
together) and raid parties (up to 8).

For this, worlds report to the directory: start the directory and each world server with the
same `VOX_WORLD_SECRET`, give each world `--directory-url http://<directory>`, and list worlds
on the directory as `--world "void=The Void=:7780"` (the id is the server's `--name`). The
operators make someone a world's owner with
`vox-directory --db directory.db --world … admin own-world void "Sam Hill"`.

**vox.fail** is on Cloudflare: a Cloudflare Tunnel carries https://vox.fail/ to the directory
and `wss://vox.fail/w/<port>/ws` to each world, with nothing open on the server. `deploy/` has
the tunnel's config, systemd units, the dashboard settings and the steps. The desktop client
goes there by default.

**Moving and building:** you start in first person, looking around with the mouse. **V** or **F5**
changes the view: first person, behind your shoulder, then from above. From above, **left click**
walks to the point under the pointer, **right click** places a block, **Shift + right click**
removes one, and the **wheel** zooms. **1–9** choose the hotbar slot. Some places have a fixed
camera that takes over while you're there. A controller: left stick walks, right stick looks,
South jumps, D-pad down flies, RT breaks, LT places, and the bumpers choose the hotbar block. The
show's hotkeys are off while you're in a world. Others appear as their own characters, moving as
they move. Every control is listed in [the player help](../help/controls.md).

**The Void** is the first world: an ethereal, flat, luminous space. A pale polished floor goes
on for ever into bright fog, three moons faint in the light. You arrive in a circle of pale
pillars with glowing tops. To the north stands the **First Gate**, a lone doorway; to the west
the black **Ember Door**. Far off in the fog: a stair to nowhere, monoliths, empty doorframes.
`tools/assets/build_void.py` makes it. The old floating islands in dark fog are now their own world
(`data/levels/islands.json`, `tools/assets/build_islands.py`).

**Portals:** walk towards the First Gate and an echo of the yard fades into it, faint and pale
at first, solid close up; walk through and you're there. The Ember Door (west) shows the
islands; the Tide Door (east) the dark sea, a stormy ocean rolling in big swells, whose Sky Door leads
up to orbit: a station 250 km above the planet. A
level lists its doorways under `"portals"` in its `"world"` object (where, which way, how big,
which level, which server, and the matching point on the far side); `":7781"` means another
port on the same host. To try them all (or run `tools/dev/play-local.sh`):

    vox-server --listen 127.0.0.1:7780 --db void.db
    vox-server --listen 127.0.0.1:7781 --db yard.db --seed data/levels/yard.json --name yard
    vox-server --listen 127.0.0.1:7782 --db islands.db --seed data/levels/islands.json --name islands
    vox-server --listen 127.0.0.1:7783 --db sea.db --seed data/levels/sea.json --name sea
    vox-server --listen 127.0.0.1:7784 --db orbit.db --seed data/levels/orbit.json --name orbit

If nobody answers on the other side, you're put back where you were. `world:portal:<name>`
goes through by command.

**Weather:** each world runs a small simulation of its air (wind, temperature, humidity,
clouds, rain), shared by everyone in it. A world without air (orbit, `"weather": "none"`) runs none
and sends none: no readout, no wind, rain or fog. It sets how far you can see, the fog round you, the
clouds, rain or snow, and which way flames and water move. The bar at the bottom shows the air
where you stand. Change it: `world:weather:storm` (clear, haze, mist, fog, luminous, rain,
storm, windy, snow), `world:wind:8:270` (m/s, from the west), `world:humidity:0.9`,
`world:temperature:-3`.

**How worlds look:** a sky drawn by a shader (sun, clouds and stars on earth; bright light in
the void; dark fog and three veiled moons on the islands), the same sky lighting everything
softly and showing in glass, metal and water, sun or moon shadows, haze as far as the weather
lets you see, sunken mortar and cobbles (parallax), moving water, flickering flames, glowing
magma, and the nearest lamps and fires lighting what's around them. On earth the time of day can
change: `world:time:18.5`, or `world:time:auto` for a day every 24 minutes; `world:clouds:0.7`.
A level chooses its own sky, time, weather, camera, block bar and portals in a `"world"`
object. Quality **high** for shadows, bloom and the volume of fog.
