import { toPng } from "html-to-image";

/** Render already-sanitized diagnosis HTML without leaving temporary DOM behind. */
export async function renderDiagnosisImage(html: string, theme: "dark" | "light"): Promise<string> {
  const node = document.createElement("section");
  node.className = `bkmsa-scope markdown-body image-export-node ${theme === "light" ? "image-export-light" : ""}`;
  node.innerHTML = html;
  document.body.appendChild(node);
  try {
    return await toPng(node, {
      cacheBust: true,
      pixelRatio: 2,
      backgroundColor: theme === "dark" ? "#10161b" : "#f8fafb",
      // Ignore both physical and logical offscreen insets in the SVG clone.
      // The live node stays fixed/offscreen so exporting does not move the page.
      style: { position: "static" },
    });
  } finally {
    node.remove();
  }
}
