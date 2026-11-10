import { readFile } from "node:fs/promises";
import { Resvg } from "@resvg/resvg-js";
import type { APIRoute } from "astro";
import satori from "satori";

// The social preview image, rendered once at build time. Paths are relative to site/, where Astro
// runs.
const read = (path: string) => readFile(new URL(path, `file://${process.cwd()}/`));

const h = (type: string, style: Record<string, unknown>, children?: unknown, props = {}) => ({
  type,
  props: { style, children, ...props },
});

// Inter has no ⌥ glyph, so the Option key draws it as a path.
const option = h("svg", { width: 38, height: 38 }, h("path", {}, undefined, {
  d: "M2 6h7l7 12h6M14 6h8",
  fill: "none",
  stroke: "#ededed",
  strokeWidth: 2.2,
  strokeLinecap: "round",
  strokeLinejoin: "round",
}), { viewBox: "0 0 24 24" });

const key = (label: unknown) =>
  h(
    "div",
    {
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
      minWidth: 64,
      height: 60,
      padding: "0 18px",
      border: "2px solid #2e2e2e",
      borderBottomWidth: 4,
      borderRadius: 12,
      background: "#1f1f1f",
      color: "#ededed",
      fontSize: 30,
    },
    label,
  );

export const GET: APIRoute = async () => {
  const [icon, inter] = await Promise.all([
    read("../assets/icon.png"),
    read("node_modules/@fontsource/inter/files/inter-latin-400-normal.woff"),
  ]);

  const svg = await satori(
    h(
      "div",
      {
        width: "100%",
        height: "100%",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        background: "#161616",
        backgroundImage: "radial-gradient(circle at 50% 0%, #262626 0%, #161616 70%)",
        color: "#ededed",
        fontFamily: "Inter",
      },
      [
        h("img", { width: 168, height: 168 }, undefined, {
          src: `data:image/png;base64,${icon.toString("base64")}`,
          width: 168,
          height: 168,
        }),
        h("div", { marginTop: 28, fontSize: 96, letterSpacing: "-0.03em", lineHeight: 1.1 }, "Runecast"),
        h(
          "div",
          { marginTop: 18, fontSize: 34, color: "#8a8a8a", letterSpacing: "-0.01em" },
          "A fast, open-source launcher for macOS and Linux",
        ),
        h("div", { display: "flex", alignItems: "center", gap: 12, marginTop: 44 }, [
          key(option),
          key("Space"),
        ]),
      ],
    ) as Parameters<typeof satori>[0],
    {
      width: 1200,
      height: 630,
      fonts: [{ name: "Inter", data: inter, weight: 400, style: "normal" }],
    },
  );

  const png = new Resvg(svg, { fitTo: { mode: "width", value: 1200 } }).render().asPng();
  return new Response(new Uint8Array(png), { headers: { "Content-Type": "image/png" } });
};
