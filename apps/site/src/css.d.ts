// Stylesheets are imported for their side effect; Bun bundles them.
declare module "*.css";

// Markdown is imported as text (`with { type: "text" }`), see features/help/pages.ts.
declare module "*.md" {
  const text: string;
  export default text;
}
