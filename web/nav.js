// The row of places across the top of the signed-in pages.
import { el } from "/vox.js";

const PLACES = [["/", "Home"], ["/create", "Character"], ["/inventory", "Inventory"], ["/friends", "Friends and raids"]];

export function nav(into) {
  const n = el("nav", { class: "nav", "aria-label": "Vox" });
  for (const [href, label] of PLACES) {
    const a = el("a", { href, text: label });
    if (location.pathname === href) a.setAttribute("aria-current", "page");
    n.append(a);
  }
  into.append(n);
}
