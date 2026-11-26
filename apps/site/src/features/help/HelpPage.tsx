// The player help: docs/help rendered one page at a time, chosen by the URL hash (#controls).

import { Anchor, Box, Flex, NavLink, Stack, Typography } from "@mantine/core";
import { useEffect, useState } from "react";
import type { Components } from "react-markdown";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Page } from "../../components/Page";
import { index, pages } from "./pages";

const REPO_DOCS = "https://github.com/johnbellone/vox.fail/blob/main/docs/";
const HELP_DIR = "/docs/help/";
// Only used to resolve relative links; the origin is never shown.
const BASE = "https://help.invalid/docs/help/";

function currentSlug(): string {
  try {
    return decodeURIComponent(location.hash.slice(1)) || index.slug;
  } catch {
    return index.slug;
  }
}

/**
 * Where a link in a help page goes. Links to another help page become hashes on this page; other
 * relative links point at the same file on GitHub, resolved from docs/help/. Only http, https and
 * mailto survive; everything else is dropped.
 */
function hrefFor(href: string | undefined, slugs: Map<string, string>): string | undefined {
  if (!href) return undefined;
  if (href.startsWith("#")) return href;
  if (/^([a-z][a-z0-9+.-]*:|\/\/)/i.test(href)) return /^(https?:|mailto:)/i.test(href) ? href : undefined;
  const url = new URL(href, BASE);
  if (url.pathname.startsWith(HELP_DIR)) {
    const name = url.pathname.slice(HELP_DIR.length);
    const slug = slugs.get(name);
    if (slug !== undefined && !name.includes("/")) return `#${slug}`;
  }
  const inDocs = url.pathname.startsWith("/docs/") ? url.pathname.slice("/docs/".length) : url.pathname.slice(1);
  return REPO_DOCS + inDocs + url.search + url.hash;
}

function componentsFor(slugs: Map<string, string>): Components {
  return {
    a: ({ href, children }) => {
      const target = hrefFor(href, slugs);
      const external = target !== undefined && /^(https?:|mailto:)/i.test(target);
      return (
        <Anchor href={target} target={external ? "_blank" : undefined} rel={external ? "noopener noreferrer" : undefined}>
          {children}
        </Anchor>
      );
    },
    // Every help page opens with its own "# Title"; the page title above already says it.
    h1: () => null,
  };
}

export function HelpPage() {
  const [slug, setSlug] = useState(currentSlug);
  useEffect(() => {
    const onHash = () => {
      setSlug(currentSlug());
      window.scrollTo(0, 0); // no element has the slug as its id, so the browser will not scroll
    };
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);

  const doc = pages.find((p) => p.slug === slug) ?? index;
  const slugs = new Map(pages.map((p) => [p.file, p.slug]));

  return (
    <Page title={doc.slug === index.slug ? "Help" : doc.title}>
      <Flex direction={{ base: "column", sm: "row" }} gap="xl" align="flex-start">
        <Stack component="nav" aria-label="Help pages" gap={2} w={{ base: "100%", sm: 220 }} style={{ flexShrink: 0 }}>
          {pages.map((p) => (
            <NavLink
              key={p.slug}
              href={`#${p.slug}`}
              label={p.title}
              active={p.slug === doc.slug}
              aria-current={p.slug === doc.slug ? "page" : undefined}
            />
          ))}
        </Stack>
        <Box style={{ flex: 1, minWidth: 0 }} maw={760}>
          <Typography>
            <ReactMarkdown remarkPlugins={[remarkGfm]} components={componentsFor(slugs)}>
              {doc.markdown}
            </ReactMarkdown>
          </Typography>
        </Box>
      </Flex>
    </Page>
  );
}
