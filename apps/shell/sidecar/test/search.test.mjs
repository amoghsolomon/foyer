import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  cacheImageResults,
  detectImageFormat,
  normalizeSearchResponse,
} from "../src/search.mjs";

test("image search keeps direct image and attribution URLs", () => {
  const [image] = normalizeSearchResponse({ results: [{
    title: "A useful diagram",
    url: "https://example.com/article",
    img_src: "https://cdn.example.com/diagram.png",
    thumbnail_src: "https://cdn.example.com/thumb.jpg",
    source: "Example",
    resolution: "1600 x 900",
  }] }, "images", 5);
  assert.equal(image.image_url, "https://cdn.example.com/diagram.png");
  assert.equal(image.page_url, "https://example.com/article");
  assert.equal(image.source, "Example");
});

test("search discards non-http and imageless image results", () => {
  const results = normalizeSearchResponse({ results: [
    { title: "Bad", url: "file:///etc/passwd", img_src: "javascript:alert(1)" },
    { title: "No image", url: "https://example.com" },
    { title: "Good", url: "https://example.com/good", thumbnail: "https://example.com/a.jpg" },
  ] }, "images", 6);
  assert.equal(results.length, 1);
  assert.equal(results[0].image_url, "https://example.com/a.jpg");
});

test("image cache validates bytes and returns a local render path", async () => {
  const directory = await mkdtemp(join(tmpdir(), "foyer-shell-images-"));
  const png = Uint8Array.from([
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3, 4,
  ]);
  try {
    const [result] = await cacheImageResults([{
      title: "Cached",
      page_url: "https://example.com/page",
      image_url: "https://cdn.example.com/image",
      thumbnail_url: null,
      source: "Example",
      resolution: "1200 x 800",
    }], directory, undefined, async () => new Response(png, {
      status: 200,
      headers: { "Content-Type": "image/png" },
    }));

    assert.match(result.image_url, /\.png$/);
    assert.deepEqual(await readFile(result.image_url), Buffer.from(png));
    assert.equal(result.page_url, "https://example.com/page");
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("image cache rejects HTML and falls back to the thumbnail", async () => {
  const directory = await mkdtemp(join(tmpdir(), "foyer-shell-images-"));
  const jpeg = Uint8Array.from([0xff, 0xd8, 0xff, 0xdb, 1, 2, 3, 4]);
  const requested = [];
  try {
    const [result] = await cacheImageResults([{
      title: "Fallback",
      page_url: "https://example.com/page",
      image_url: "https://cdn.example.com/full",
      thumbnail_url: "https://cdn.example.com/thumb",
      source: "Example",
      resolution: null,
    }], directory, undefined, async (url) => {
      requested.push(url);
      return url.endsWith("/full")
        ? new Response("<html>blocked</html>", { status: 200 })
        : new Response(jpeg, { status: 200 });
    });

    assert.deepEqual(requested, [
      "https://cdn.example.com/full",
      "https://cdn.example.com/thumb",
    ]);
    assert.match(result.image_url, /\.jpg$/);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("image format detection rejects non-image payloads", () => {
  assert.equal(detectImageFormat(new TextEncoder().encode("<html>not an image</html>")), null);
  assert.equal(detectImageFormat(new TextEncoder().encode("<svg viewBox='0 0 10 10'></svg>")), "svg");
});
