# Vox on Steam

Steam installs, updates and uninstalls the game, so there is no installer: each platform's
depot is the client and the files it looks for beside it (`find_root` in
`bins/vox-client/src/main.rs`):

```
vox-client(.exe)
assets/
data/
```

`.github/workflows/steam.yml` builds the Windows and Linux depots for every `v*` tag, the same
tag `release.yml` puts live on vox.fail, so the Steam client always matches the servers. It
uploads them to a test branch (`beta`). Setting a build live for players is done by hand.

Block Avatar is a separate program and is not part of this app.

## Setting it up (once)

### Steamworks (partner.steamgames.com)

1. **The app.** Pay the Steam Direct fee and create the app. Note its **App ID**.
2. **Depots** (SteamPipe → Depots). Make two, and note their ids:
   - Windows: Operating System **Windows**.
   - Linux: Operating System **Linux + SteamOS**. The Steam Deck uses this one.
3. **Launch options** (Installation → General Installation):
   - Windows: executable `vox-client.exe`, Operating System Windows.
   - Linux: executable `vox-client`, Operating System Linux + SteamOS.
4. **The Linux runtime**: choose **Steam Linux Runtime 3.0 (sniper)** for the app (under
   Installation). The workflow builds in that runtime's SDK, so the libraries match.
5. **Redistributables** (Installation → Redistributables): tick **Visual C++ Redist 2022**.
   Rust programs built for Windows need `vcruntime140.dll`.
6. **The test branch** (SteamPipe → Builds): add a branch named `beta`. A password on it keeps
   it to testers.
7. **Publish** (Publish → Prepare for Publishing). Nothing above takes effect until you do.

### A build account

The workflow signs in as a Steam account of its own, never yours:

1. Make a new Steam account for building.
2. In Steamworks (Users & Permissions → Manage Users), invite it with only **Edit App
   Metadata** and **Publish App Changes To Steam**, for this app only.
3. On any machine with steamcmd, sign in once: `steamcmd +login <account> +quit`. It asks for
   the password and the Steam Guard code (from the account's email, or the Steam app if it
   has the mobile authenticator).
4. That leaves a saved login in steamcmd's `config/config.vdf`. Encode it for GitHub:
   `base64 -w0 config/config.vdf`.

The saved login lasts a while but not forever. When it runs out, the upload fails with
"steamcmd did not report a finished build": repeat steps 3 and 4 and replace the secret.
steamcmd's own logs are kept as the `steam-build-logs` artifact when the upload fails.

### GitHub

1. **The environment.** Settings → Environments → New environment **`steam`**. Under
   Deployment branches and tags, allow only tags matching `v*`.
2. **Its secrets:**
   - `STEAM_USERNAME`: the build account's name.
   - `STEAM_CONFIG_VDF`: the base64 text from step 4 above.
3. **Repository variables** (Settings → Secrets and variables → Actions → Variables). They are
   the repository's, not the environment's, because the upload job checks `STEAM_APP_ID`
   before it enters the environment:
   - `STEAM_APP_ID`, `STEAM_DEPOT_WINDOWS`, `STEAM_DEPOT_LINUX`: the ids from Steamworks.
   - `STEAM_BRANCH` (optional): the test branch, `beta` if unset.

Until `STEAM_APP_ID` is set, the workflow still builds both depots and keeps them as artifacts
(`vox-client-windows`, `vox-client-linux`) to try, and skips the upload. A manual run from a
branch (Actions → Steam → Run workflow) does the same: only tags upload.

## Releasing

1. Tag a version (`v0.2.0`). `release.yml` deploys vox.fail; `steam.yml` uploads the client to
   `beta`.
2. In the Steam client: the game's Properties → Betas → `beta`. Play it on Windows, Linux and
   a Steam Deck.
3. In Steamworks (SteamPipe → Builds), set the build live on **default**. `upload.sh` refuses
   to do this itself.

To try the upload by hand, `deploy/steam/upload.sh --dry-run` prints the build script it would
run (its header lists the arguments). Before the first real upload, it's worth running that
script once through `steamcmd +login <account> +run_app_build -preview <script> +quit`, which
lists what would go in each depot without uploading.

## Not done yet

- **Signing in from the game.** The client still needs a launch code copied from the site
  (`--token`). On Steam it should sign in with a Steam ticket, and the directory should check
  that ticket with Steam. That needs a publisher Web API key, not the `VOX_STEAM_KEY` the
  site's sign-in uses. Until then, a Steam player copies a launch code from the site into the
  game's launch options (Properties → General → Launch Options: `--token <code>`), and again
  each day, since the token lasts a day.
- **The Steam Deck.** Show Steam's on-screen keyboard where players type, check the HUD at
  1280×800, then ask for the Deck compatibility review in Steamworks.
- **macOS.** There is no macOS build. It needs a universal `.app`, a microphone usage
  description (for voice) and signing.
- **Size.** The depots carry all of `assets/` (about 117 MB), Block Avatar's files included.
- **Pinning.** The workflow uses the runtime SDK's `latest` image. Pin it to a dated version
  once one has built.
