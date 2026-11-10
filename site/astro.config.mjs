import sitemap from "@astrojs/sitemap";
import { defineConfig } from "astro/config";

export default defineConfig({
  site: "https://runecast.dinogomez.app",
  integrations: [sitemap({ filter: (page) => !page.endsWith(".png") })],
  devToolbar: { enabled: false },
  // The page imports the app icon and screenshots from ../assets and ../docs, so the README and
  // the site share one copy of each.
  vite: { server: { fs: { allow: [".."] } } },
});
