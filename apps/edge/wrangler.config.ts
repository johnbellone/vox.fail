// Where the offline page's files are (bun run build writes them).
import { defineWranglerConfig } from "wrangler/experimental-config";

export default defineWranglerConfig({
	assetsDirectory: "dist",
});
