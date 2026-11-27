// The WebAssembly client (vox-client built by tools/dev/build_web.sh into public/pkg). It is loaded
// at run time, not bundled, and reports how it's getting on through three window callbacks.

declare global {
  interface Window {
    vox_status?: (text: string) => void;
    vox_ready?: () => void;
    vox_error?: (text: string) => void;
    /** the editor's events, as JSON text (see editor.ts) */
    vox_editor?: (json: string) => void;
  }
}

/** What `start` takes (client/src/lib.rs, `Start`). */
export interface StartConfig {
  canvas: string;
  url: string;
  token?: string;
  name?: string;
  model?: string;
  wear?: string[];
  /** look at the world without joining it: no sign-in, no avatar, no controls */
  watch?: boolean;
  /** start in edit mode (the world editor); the client refuses commands otherwise */
  edit?: boolean;
  base?: string;
}

interface Client {
  default: () => Promise<unknown>;
  start: (config: string) => void;
  /** the editor's command entry point (client/src/lib.rs, `editor_command`) */
  editor_command?: (json: string) => void;
}

// where the client is served; a variable so the bundler leaves the import alone
const CLIENT_URL = "/pkg/vox_client.js";

// the loaded client, for the editor's commands; undefined until loadClient has run
let loaded: Client | undefined;

export async function loadClient(): Promise<(config: StartConfig) => void> {
  let client: Client;
  try {
    client = (await import(/* @vite-ignore */ CLIENT_URL)) as Client;
    await client.default();
  } catch (err) {
    console.error(err);
    throw new Error("the browser client isn't installed on this server yet (tools/dev/build_web.sh builds it)");
  }
  loaded = client;
  return (config) => client.start(JSON.stringify({ base: location.origin, ...config }));
}

/** Sends one editor command (JSON text) to the running client. Throws when no client is loaded
 *  or it predates the editor. */
export function editorCommand(json: string): void {
  const send = loaded?.editor_command;
  if (!send) throw new Error("the editor isn't running");
  send(json);
}

interface Gpu {
  requestAdapter: () => Promise<unknown>;
}

/**
 * Why this browser can't run the client, or undefined when it can. The client draws with WebGPU
 * only; without a GPU to give, it would stop silently at "Starting…". `ua` and `gpu` are
 * parameters so the messages can be tested.
 */
export async function webgpuProblem(
  ua: string = navigator.userAgent,
  gpu: Gpu | undefined = (navigator as Navigator & { gpu?: Gpu }).gpu,
): Promise<string | undefined> {
  const firefox = /Firefox\//.test(ua);
  const elsewhere = "play in Chrome or Edge, or use the desktop client";
  if (!gpu) {
    return firefox
      ? `Vox draws with WebGPU, which isn't switched on in this Firefox. Turn on dom.webgpu.enabled in about:config and reload, or ${elsewhere}.`
      : `Vox draws with WebGPU, which this browser doesn't offer. Update it, or ${elsewhere}.`;
  }
  let adapter: unknown = null;
  try {
    adapter = await gpu.requestAdapter();
  } catch {
    adapter = null;
  }
  if (adapter == null) {
    return `This browser has WebGPU but couldn't get a graphics card for it (it may be blocked for this driver). Check that hardware acceleration is on, or ${elsewhere}.`;
  }
  return undefined;
}

/** Can this browser run the client well enough to show it unasked? Checked before downloading
 *  it: a browser may offer WebGPU yet have no GPU to give (the client would then stop). */
export async function canShowLive(): Promise<boolean> {
  if (typeof navigator === "undefined") return false;
  if (matchMedia("(prefers-reduced-motion: reduce)").matches) return false;
  // small screens are mostly phones: not megabytes of client before anyone asked to play
  if (matchMedia("(max-width: 767px)").matches) return false;
  const conn = (navigator as Navigator & { connection?: { saveData?: boolean } }).connection;
  if (conn?.saveData) return false;
  const gpu = (navigator as Navigator & { gpu?: Gpu }).gpu;
  if (!gpu) return false;
  try {
    return (await gpu.requestAdapter()) != null;
  } catch {
    return false;
  }
}
