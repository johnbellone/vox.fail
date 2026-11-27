// Which of the container's servers a path goes to: the worlds (vox-host) or the directory.
export const DIRECTORY_PORT = 7700;
export const HOST_PORT = 7790;
export const portFor = (path: string) => (path.startsWith("/w/") ? HOST_PORT : DIRECTORY_PORT);

/**
 * The visitor's request, addressed to the container: the servers inside speak plain HTTP, so a
 * request still addressed https://vox.fail/... would be tried over TLS and fail. Method, headers
 * and body (and a socket upgrade) go with it; only the address changes.
 */
export function toContainer(req: Request): Request {
  const url = new URL(req.url);
  return new Request(`http://container${url.pathname}${url.search}`, req);
}
