# Getting started

You can play Vox in the browser or on the desktop.

## In the browser

1. Open `https://vox.fail/`. The browser needs WebGPU (current Chrome, Edge, Safari and Firefox
   have it).
2. Sign in with Steam or Discord (when the site offers them), or make a **New account** with a name and a password.
3. Pick who you are (38 characters, Blue first) and what you wear: a hat, something on your
   face, something in your hand. You see it on a turntable.
4. Click **Save character**, then **Enter the Void**.

## On the desktop

### Download

The desktop client is on the [GitHub releases page](https://github.com/johnbellone/vox.fail/releases).
Each release has these files, where `<version>` is the version number:

- Windows: `Vox-<version>-windows-x64-setup.exe` (an installer) or `Vox-<version>-windows-x64.zip` (no install).
- macOS: `Vox-<version>-macos-universal.dmg`.
- Linux: `Vox-<version>-linux-x86_64.AppImage` or `Vox-<version>-linux-x86_64.tar.gz`.

The `nightly` release is an unsigned test build of the main branch, rebuilt each day when it changes. It may break. See
[releases](../vox/releases.md) for how the builds are made.

The files are not always signed, so your system may warn you the first time you open them:

- Windows: SmartScreen may stop it. Choose **More info**, then **Run anyway**.
- macOS: open the disk image and drag Vox into Applications. Then right-click Vox and choose **Open**,
  then confirm. On newer macOS, if that does not work, open System Settings > Privacy & Security and
  choose **Open Anyway**.

The client has no box for a sign-in code yet. Start it from a terminal, or from a shortcut, with
`--token <code>`:

- Linux AppImage: first make it runnable with `chmod +x Vox-<version>-linux-x86_64.AppImage`, then run
  `./Vox-<version>-linux-x86_64.AppImage --token <code>`. From the `.tar.gz`, unpack it, go into the
  folder and run `./vox-client --token <code>`.
- Windows: open `%LOCALAPPDATA%\Programs\Vox` (the installer's folder) and run
  `vox-client.exe --token <code>` there.
- macOS: after you drag Vox to Applications, run
  `/Applications/Vox.app/Contents/MacOS/vox-client --token <code>`.

Your key bindings (`controls.json`) and your own levels (in `levels/`) are kept apart from the
program, in a Vox folder for you: `%APPDATA%\Vox` on Windows, `~/Library/Application Support/Vox` on
macOS, and `$XDG_DATA_HOME/vox` (`~/.local/share/vox`) on Linux.

### Run from the source

Run the local worlds from the repository root. This starts the Void, the yard and the islands
on your machine, with guests allowed, and opens the client in the Void:

```bash
tools/dev/play-local.sh
tools/dev/play-local.sh --name Sam --model models/dog.glb   # your name and character
```

The first run builds in release mode. The first build compiles Bevy and takes several minutes.

The script refuses to start if ports 7780 to 7782 are taken. A leftover server is the usual
cause. Clear it with:

```bash
pkill vox-server
```

To join with a sign-in code from the start page instead, copy the code and run:

```bash
vox-client --token <code>
```

The code lasts a day. To join another server, add `--world ws://host:7780/ws`.

## Next

- [Controls](controls.md): how to move, build and use the menus.
- [Troubleshooting](troubleshooting.md): when something does not work.
