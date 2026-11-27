# Getting started

You can play Vox in the browser or on the desktop.

## In the browser

1. Open `https://vox.fail/`. The browser needs WebGPU (current Chrome, Edge, Safari and Firefox
   have it).
2. Sign in with Steam or Discord (when the site offers them), or make a **New account** with a name and a password.
3. Pick who you are (39 characters, Blue first) and what you wear: a hat, something on your
   face, something in your hand. You see it on a turntable.
4. Click **Save character**, then **Enter the Void**.

## On the desktop

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
