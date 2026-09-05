import { afterEach, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const styles = readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), "../src/styles.css"), "utf8");

afterEach(() => {
  document.head.querySelectorAll("[data-spark-style-test]").forEach((node) => node.remove());
  document.body.replaceChildren();
});

function installStyles() {
  const style = document.createElement("style");
  style.dataset.sparkStyleTest = "";
  style.textContent = styles;
  document.head.append(style);
}

it("does not change host headings, generic layout classes or root theme variables", () => {
  const host = document.createElement("section");
  host.innerHTML = '<h2>Host heading</h2><div class="panel">Host panel</div><div class="app-shell">Host shell</div>';
  document.body.append(host);
  const heading = host.querySelector("h2")!;
  const panel = host.querySelector(".panel")!;
  const shell = host.querySelector(".app-shell")!;
  const baseline = {
    headingSize: getComputedStyle(heading).fontSize,
    headingMargin: getComputedStyle(heading).margin,
    panelPadding: getComputedStyle(panel).padding,
    shellOverflow: getComputedStyle(shell).overflow,
    rootText: getComputedStyle(document.documentElement).getPropertyValue("--text"),
  };
  installStyles();
  expect({
    headingSize: getComputedStyle(heading).fontSize,
    headingMargin: getComputedStyle(heading).margin,
    panelPadding: getComputedStyle(panel).padding,
    shellOverflow: getComputedStyle(shell).overflow,
    rootText: getComputedStyle(document.documentElement).getPropertyValue("--text"),
  }).toEqual(baseline);
});

it("styles analyzer roots, descendants, teleported diagnosis and image exports", () => {
  document.body.innerHTML = `
    <div class="bkmsa-scope app-shell"><h2>Analyzer</h2><div class="panel">Panel</div></div>
    <div class="bkmsa-scope fullscreen-diagnosis" data-theme="light"><h2>Diagnosis</h2></div>
    <div class="bkmsa-scope markdown-body image-export-node"><h2>Export</h2></div>
  `;
  installStyles();
  const app = document.querySelector(".app-shell")!;
  const fullscreen = document.querySelector(".fullscreen-diagnosis")!;
  const exported = document.querySelector(".image-export-node")!;
  expect(getComputedStyle(app).display).toBe("flex");
  expect(getComputedStyle(app).getPropertyValue("--text").trim()).toBe("#dce4ea");
  expect(getComputedStyle(app.querySelector("h2")!).fontSize).toBe("15px");
  expect(getComputedStyle(app.querySelector(".panel")!).padding).toBe("14px");
  expect(getComputedStyle(fullscreen).display).toBe("flex");
  expect(getComputedStyle(fullscreen).getPropertyValue("--text").trim()).toBe("#202830");
  expect(getComputedStyle(exported).position).toBe("fixed");
  expect(getComputedStyle(exported.querySelector("h2")!).fontSize).toBe("18px");
});
