// Throwaway visual spike: screenshot github.com comments, signed out, with the host's Chromium.
//   node capture.mjs SHOTS_JSON OUT_DIR
// SHOTS_JSON: [{ "name": "x.png", "url": "<comment html_url>", "width": 390, "expand": false }]
// `expand: true` clicks every collapsed <summary> in that comment, the way a reader would.
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_MODULE ?? "playwright");
const [shotsPath, outDir] = process.argv.slice(2);
const shots = JSON.parse(readFileSync(shotsPath, "utf8"));
const browser = await chromium.launch();
try {
	for (const shot of shots) {
		const page = await browser.newPage({
			viewport: { width: shot.width, height: 900 },
			deviceScaleFactor: 2,
			colorScheme: "light",
		});
		const id = new URL(shot.url).hash.slice(1);
		await page.goto(shot.url, { waitUntil: "domcontentloaded", timeout: 60_000 });
		const comment = page.locator(`[id="${id}"]`).first();
		await comment.waitFor({ timeout: 60_000 });
		if (shot.expand) {
			const closed = comment.locator(".comment-body details:not([open]) > summary");
			while ((await closed.count()) > 0) await closed.first().click();
		}
		await page.waitForTimeout(500);
		await comment.screenshot({ path: join(outDir, shot.name) });
		await page.close();
		console.error(`captured ${shot.name}`);
	}
} finally {
	await browser.close();
}
