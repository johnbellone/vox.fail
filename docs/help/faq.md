# FAQ

## Does Vox run in a browser?

Yes. The same client that runs on the desktop is also built for WebAssembly, so it runs in
the browser. You need WebGPU. Current Chrome, Edge, Safari and Firefox have it.

## How do I sign in?

Open the start page and choose **Sign in with Steam**, **Sign in with Discord**, or **New
account** with a name and password. Then pick your character and choose **Enter the Void**.

## How do I play on the desktop?

Copy your launch code from the start page and run `vox-client --token <code>`. The code lasts
a day. To connect to another server, add `--world ws://host:7780/ws`.

## Where are worlds saved?

When you run worlds locally, they save to `void.db`, `yard.db` and `islands.db` in the
repository root. Git ignores these files.

## What are the worlds?

The Void, the yard and the islands. Walk through a portal to reach another world. Portals
name their destination by port.

## What do the button hints show?

The hints draw the button you press. On a controller, they show the button as it looks on
the controller in your hands. On a keyboard, they show the key. They change as soon as you
switch device or rebind a control.

## Can I use a virtual webcam on Linux?

No. The `W` virtual camera is Windows-only. On Linux, use OBS window capture, or OBS's own
virtual camera. See [Troubleshooting](troubleshooting.md).
