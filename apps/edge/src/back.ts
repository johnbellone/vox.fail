// Where the offline page's "Try again" goes: back to the page that sent it there (the site's
// API client adds ?back=<path>), but only to a page of this site.

/** `origin` is this site's (location.origin). Anything else, or anything odd, goes to "/". */
export function backTo(search: string, origin: string): string {
  const back = new URLSearchParams(search).get("back") ?? "/";
  // Browsers drop tabs and newlines from addresses, so "/\t/evil.com" would become
  // "//evil.com": no control characters, spaces or backslashes at all.
  if (!back.startsWith("/") || /[\u0000- \u007f\\]/.test(back)) return "/";
  let u: URL;
  try {
    u = new URL(back, origin);
  } catch {
    return "/";
  }
  if (u.origin !== origin || u.pathname.startsWith("/_offline/")) return "/";
  return u.pathname + u.search + u.hash;
}
