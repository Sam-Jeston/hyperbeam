// The Hyperbeam mark: a 16x16 pixel beam fired corner to corner from a charged orb.
// One path per layer, one unit per pixel. Shared by web/, site/, docs/ and the favicon.
// Layers, outside in: beam edge (primary), beam body (second), glow, white-hot core.

export const MARK_LAYERS = {
  edge: "M12 0h1v1h-1zM11 1h1v1h-1zM10 2h1v1h-1zM9 3h1v1h-1zM15 3h1v1h-1zM8 4h1v1h-1zM14 4h1v1h-1zM7 5h1v1h-1zM13 5h1v1h-1zM6 6h1v1h-1zM12 6h1v1h-1zM4 7h2v1h-2zM11 7h1v1h-1zM2 8h3v1h-3zM10 8h1v1h-1zM1 9h2v1h-2zM9 9h1v1h-1zM1 10h1v1h-1zM8 10h1v1h-1zM0 11h2v1h-2zM7 11h2v1h-2zM1 12h1v1h-1zM7 12h1v1h-1zM1 13h2v1h-2zM6 13h2v1h-2zM2 14h5v1h-5zM4 15h1v1h-1z",
  body: "M13 0h1v1h-1zM12 1h1v1h-1zM8 2h1v1h-1zM11 2h1v1h-1zM15 2h1v1h-1zM10 3h1v1h-1zM14 3h1v1h-1zM9 4h1v1h-1zM13 4h1v1h-1zM8 5h1v1h-1zM12 5h1v1h-1zM7 6h1v1h-1zM11 6h1v1h-1zM6 7h1v1h-1zM10 7h1v1h-1zM5 8h1v1h-1zM9 8h1v1h-1zM13 8h1v1h-1zM3 9h2v1h-2zM8 9h1v1h-1zM2 10h1v1h-1zM7 10h1v1h-1zM2 11h1v1h-1zM6 11h1v1h-1zM2 12h1v1h-1zM6 12h1v1h-1zM3 13h3v1h-3z",
  glow: "M2 1h1v1h-1zM1 2h1v1h-1zM14 14h1v1h-1z",
  core: "M14 0h2v1h-2zM13 1h3v1h-3zM12 2h3v1h-3zM11 3h3v1h-3zM10 4h3v1h-3zM9 5h3v1h-3zM8 6h3v1h-3zM7 7h3v1h-3zM6 8h3v1h-3zM5 9h3v1h-3zM3 10h4v1h-4zM3 11h3v1h-3zM3 12h3v1h-3z",
} as const;

/** Theme variables for each layer, in paint order. */
export const MARK_FILLS: [keyof typeof MARK_LAYERS, string][] = [
  ["edge", "var(--primary)"],
  ["body", "var(--second)"],
  ["glow", "var(--glow)"],
  ["core", "var(--core)"],
];

/** The mark on the dark surface as a data: URL, for favicons. Colors are the dark theme's. */
export const FAVICON = (() => {
  const hex = { edge: "#b98ae0", body: "#ee7c9c", glow: "#d9bdf3", core: "#fbf7ff" };
  const paths = (Object.keys(hex) as (keyof typeof hex)[]).map((k) => `<path fill='${hex[k]}' d='${MARK_LAYERS[k]}'/>`).join("");
  const svg = `<svg xmlns='http://www.w3.org/2000/svg' viewBox='-2 -2 20 20' shape-rendering='crispEdges'><rect x='-2' y='-2' width='20' height='20' rx='4' fill='#161d19'/>${paths}</svg>`;
  return "data:image/svg+xml," + encodeURIComponent(svg).replace(/%20/g, " ").replace(/%3D/g, "=").replace(/%3A/g, ":").replace(/%2F/g, "/").replace(/%27/g, "'");
})();
