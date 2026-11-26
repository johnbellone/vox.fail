# Troubleshooting

Each entry gives the symptom, the cause, and the fix.

## "Port 7780, 7781 or 7782 is taken" (play-local.sh will not start)

**Cause:** an earlier world server is still running. `tools/dev/play-local.sh` refuses to
start while ports 7780 to 7782 are in use.

**Fix:** stop the old servers, then start again:

```bash
pkill vox-server
```

## The first build takes several minutes

**Cause:** the first `cargo build` compiles Bevy, the game engine. It takes about five minutes.

**Fix:** wait. Later builds are much faster. To check the code without building the game,
run `cargo check --workspace`.

## Linux build fails on missing audio or input headers

**Cause:** the audio and input libraries need their development headers.

**Fix:**

- Debian or Ubuntu: `sudo apt install libasound2-dev libudev-dev`
- Arch: `sudo pacman -S alsa-lib systemd-libs`

## The browser client does not start

**Cause:** the browser client needs WebGPU.

**Fix:** use a current version of Chrome, Edge, Safari or Firefox. These have WebGPU. Then
open the page again.

## My launch code does not work

**Cause:** a launch code lasts a day. An older code is no longer valid.

**Fix:** open the start page again and copy a new code. Then run
`vox-client --token <code>`.

## I walked into a portal and I am back where I started

**Cause:** the destination world could not be reached. The client then returns you to the
world you came from.

**Fix:** the destination is probably down or not running. Try again later. Developers can
check the world's port and its `url` in the level file.

## Linux has no virtual webcam output

**Cause:** the `W` virtual camera is Windows-only. It writes to OBS's DirectShow queue, and
Linux has no such queue.

**Fix:** use OBS window capture to show the app. If you need a camera device, use OBS's own
virtual camera.
