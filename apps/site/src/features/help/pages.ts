// The help pages, read from docs/help/ when the site is bundled. The markdown is imported as
// text, so the site keeps no copy of it: edit the markdown files, not this list.

import readme from "../../../../../docs/help/README.md" with { type: "text" };
import gettingStarted from "../../../../../docs/help/getting-started.md" with { type: "text" };
import controls from "../../../../../docs/help/controls.md" with { type: "text" };
import voice from "../../../../../docs/help/voice.md" with { type: "text" };
import building from "../../../../../docs/help/building.md" with { type: "text" };
import worlds from "../../../../../docs/help/worlds.md" with { type: "text" };
import accounts from "../../../../../docs/help/accounts.md" with { type: "text" };
import avatar from "../../../../../docs/help/avatar.md" with { type: "text" };
import troubleshooting from "../../../../../docs/help/troubleshooting.md" with { type: "text" };
import faq from "../../../../../docs/help/faq.md" with { type: "text" };

export interface HelpDoc {
  /** The file name inside docs/help/, used to rewrite links between pages. */
  file: string;
  /** The URL hash that shows the page: #controls. The README is "index". */
  slug: string;
  /** The first "# " heading of the markdown. */
  title: string;
  markdown: string;
}

function doc(file: string, slug: string, markdown: string): HelpDoc {
  const heading = markdown.split("\n").find((line) => line.startsWith("# "));
  if (heading === undefined) throw new Error(`docs/help/${file} has no "# " heading`);
  return { file, slug, title: heading.slice(2).trim(), markdown };
}

/** The contents page. The default when the URL has no hash. */
export const index = doc("README.md", "index", readme);

export const pages: HelpDoc[] = [
  index,
  doc("getting-started.md", "getting-started", gettingStarted),
  doc("controls.md", "controls", controls),
  doc("voice.md", "voice", voice),
  doc("building.md", "building", building),
  doc("worlds.md", "worlds", worlds),
  doc("accounts.md", "accounts", accounts),
  doc("avatar.md", "avatar", avatar),
  doc("troubleshooting.md", "troubleshooting", troubleshooting),
  doc("faq.md", "faq", faq),
];
