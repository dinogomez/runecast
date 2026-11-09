import { defineConfig } from "astro/config";

export default defineConfig({
  devToolbar: { enabled: false },
  // The page imports the app icon and screenshots from ../assets and ../docs, so the README and
  // the site share one copy of each.
  vite: { server: { fs: { allow: [".."] } } },
});
