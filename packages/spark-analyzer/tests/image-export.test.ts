import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { toPng } from "html-to-image";
import { renderDiagnosisImage } from "../src/image-export";

// Exercise the real clone/style/SVG pipeline; jsdom cannot rasterize an SVG image.
vi.mock("html-to-image", async (importOriginal) => {
  const original = await importOriginal<typeof import("html-to-image")>();
  return {
    ...original,
    toPng: vi.fn((node: HTMLElement, options: Parameters<typeof original.toSvg>[1]) => {
      expect(node.isConnected).toBe(true);
      expect(getComputedStyle(node).left).toBe("-12000px");
      return original.toSvg(node, options);
    }),
  };
});

beforeEach(() => {
  // jsdom omits this constructor; the fixture contains no SVG image elements.
  vi.stubGlobal("SVGImageElement", class extends SVGElement {});
  const css = document.createElement("style");
  css.textContent = readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), "../src/styles.css"), "utf8");
  // WebKit serializes physical offsets as logical insets too; left alone is insufficient.
  css.textContent += "\n.image-export-node { inset-inline-start: -12000px; }";
  css.dataset.imageExportTest = "";
  document.head.append(css);
  const computedStyle = window.getComputedStyle.bind(window);
  // jsdom has no pseudo-element styles. Normal element styles remain real.
  vi.spyOn(window, "getComputedStyle").mockImplementation((element) => computedStyle(element));
});

afterEach(() => {
  document.head.querySelectorAll("[data-image-export-test]").forEach((node) => node.remove());
  document.body.replaceChildren();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.mocked(toPng).mockClear();
});

it.each(["light", "dark"] as const)("places the %s cloned diagnosis inside the image canvas", async (theme) => {
  const dataUrl = await renderDiagnosisImage("<h2>Diagnosis</h2><p>Visible fixture</p>", theme);
  const svg = new DOMParser().parseFromString(decodeURIComponent(dataUrl.split(",")[1]!), "image/svg+xml");
  const clone = svg.querySelector<HTMLElement>("foreignObject > section");
  expect(clone).not.toBeNull();
  expect(clone?.textContent).toContain("Visible fixture");
  expect(clone?.style.position).toBe("static");
  expect(document.querySelector(".image-export-node")).toBeNull();
});

it("removes the offscreen node when rendering fails", async () => {
  vi.mocked(toPng).mockRejectedValueOnce(new Error("render failed"));
  await expect(renderDiagnosisImage("<p>Fixture</p>", "light")).rejects.toThrow("render failed");
  expect(document.querySelector(".image-export-node")).toBeNull();
});
