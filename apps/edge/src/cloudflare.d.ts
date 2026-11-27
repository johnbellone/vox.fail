// The small part of the Workers runtime's types that src/container.ts uses (Durable Objects and
// the Container API, https://developers.cloudflare.com/containers/api/durable-object-container/),
// so the package needs no @cloudflare/workers-types, whose globals clash with the DOM's.

interface WorkerFetcher {
  fetch(input: Request | string, init?: RequestInit): Promise<Response>;
}

interface ContainerStartOptions {
  image?: string;
  instance?: "lite" | "basic" | "standard-1" | "standard-2" | "standard-3" | "standard-4";
  enableInternet?: boolean;
  entrypoint?: string[];
  env?: Record<string, string>;
}

interface Container {
  readonly running: boolean;
  readonly images: Record<string, string>;
  start(options?: ContainerStartOptions): void;
  monitor(): Promise<void>;
  signal(signo: number): void;
  destroy(reason?: unknown): Promise<void>;
  getTcpPort(port: number): WorkerFetcher;
  setInactivityTimeout(ms: number): Promise<void>;
}

interface DurableObjectStorage {
  setAlarm(scheduledTime: number): Promise<void>;
}

interface DurableObjectState {
  readonly container?: Container;
  readonly storage: DurableObjectStorage;
  blockConcurrencyWhile<T>(fn: () => Promise<T>): Promise<T>;
  waitUntil(p: Promise<unknown>): void;
}

interface DurableObjectId {}
interface DurableObjectStub extends WorkerFetcher {}
interface DurableObjectNamespace {
  idFromName(name: string): DurableObjectId;
  get(id: DurableObjectId): DurableObjectStub;
}

// WebSockets as Workers have them: accept() to handle one here, a pair to hand one to a client.
interface WebSocket {
  accept(): void;
}
declare class WebSocketPair {
  0: WebSocket;
  1: WebSocket;
}
interface Response {
  readonly webSocket: WebSocket | null;
}
interface ResponseInit {
  webSocket?: WebSocket | null;
}

declare module "cloudflare:workers" {
  export abstract class DurableObject<Env = unknown> {
    protected ctx: DurableObjectState;
    protected env: Env;
    constructor(ctx: DurableObjectState, env: Env);
    fetch?(req: Request): Promise<Response>;
    alarm?(): Promise<void>;
  }
}
