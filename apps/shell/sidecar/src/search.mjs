import { createHash } from "node:crypto";
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

const MAX_RESULTS = 6;
const MAX_IMAGE_BYTES = 16 * 1024 * 1024;
const imageCache = new Map();

export function normalizeSearchResponse(payload, mode = "references", limit = MAX_RESULTS) {
  const results = Array.isArray(payload?.results) ? payload.results : [];
  return results.flatMap((result) => {
    if (!result || typeof result !== "object") return [];
    const pageUrl = safeHttpUrl(result.url);
    if (!pageUrl) return [];
    if (mode === "images") {
      const imageUrl = safeHttpUrl(result.img_src);
      const thumbnailUrl = safeHttpUrl(result.thumbnail_src ?? result.thumbnail);
      if (!imageUrl && !thumbnailUrl) return [];
      return [{
        title: clean(result.title, 120),
        page_url: pageUrl,
        image_url: imageUrl ?? thumbnailUrl,
        thumbnail_url: thumbnailUrl,
        source: clean(result.source ?? result.engine, 80),
        resolution: clean(result.resolution, 32),
      }];
    }
    return [{
      title: clean(result.title, 140),
      url: pageUrl,
      snippet: clean(result.content, 320),
      source: clean(result.source ?? result.engine, 80),
      published_at: result.publishedDate ?? result.pubdate ?? null,
    }];
  }).slice(0, Math.max(1, Math.min(MAX_RESULTS, limit)));
}

export async function searchSearxng(baseUrl, query, mode, limit, signal, imageCacheDir = null) {
  const endpoint = new URL("/search", `${baseUrl.replace(/\/$/, "")}/`);
  endpoint.searchParams.set("q", query);
  endpoint.searchParams.set("format", "json");
  endpoint.searchParams.set("safesearch", "1");
  endpoint.searchParams.set("categories", mode === "images" ? "images" : "general");
  const response = await fetch(endpoint, {
    headers: { Accept: "application/json" },
    signal,
  });
  if (!response.ok) throw new Error(`SearXNG returned HTTP ${response.status}`);
  const results = normalizeSearchResponse(await response.json(), mode, limit);
  if (mode !== "images" || !imageCacheDir) return results;
  return cacheImageResults(results, imageCacheDir, signal);
}

export async function cacheImageResults(results, cacheDir, signal, fetcher = fetch) {
  await mkdir(cacheDir, { recursive: true });
  const cached = await Promise.all(results.map(async (result) => {
    const candidates = [...new Set([result.image_url, result.thumbnail_url].filter(Boolean))];
    for (const candidate of candidates) {
      try {
        const path = await cacheImage(candidate, result.page_url, cacheDir, signal, fetcher);
        return {
          title: result.title,
          page_url: result.page_url,
          image_url: path,
          source: result.source,
          resolution: result.resolution,
        };
      } catch (error) {
        const host = new URL(candidate).hostname;
        console.error(
          `Foyer Shell image cache skipped ${host}: ${error instanceof Error ? error.message : String(error)}`,
        );
      }
    }
    return null;
  }));
  return cached.filter(Boolean);
}

async function cacheImage(url, pageUrl, cacheDir, signal, fetcher) {
  const existing = imageCache.get(url);
  if (existing) return existing;

  const response = await fetcher(url, {
    redirect: "follow",
    headers: {
      Accept: "image/avif,image/webp,image/png,image/jpeg,image/*;q=0.9,*/*;q=0.4",
      Referer: pageUrl,
      "User-Agent": "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 Chrome/128 Safari/537.36 FoyerShell/0.1",
    },
    signal,
  });
  if (!response.ok) throw new Error(`Image returned HTTP ${response.status}`);
  const declaredLength = Number(response.headers.get("content-length"));
  if (Number.isFinite(declaredLength) && declaredLength > MAX_IMAGE_BYTES) {
    throw new Error("Image exceeds the cache size limit");
  }

  const bytes = await readLimitedBody(response, MAX_IMAGE_BYTES);
  const extension = detectImageFormat(bytes);
  if (!extension) throw new Error("Response was not a supported image");

  const digest = createHash("sha256").update(url).digest("hex").slice(0, 24);
  const path = join(cacheDir, `${digest}.${extension}`);
  await writeFile(path, bytes);
  imageCache.set(url, path);
  return path;
}

async function readLimitedBody(response, maximum) {
  if (!response.body) throw new Error("Image response had no body");
  const reader = response.body.getReader();
  const chunks = [];
  let length = 0;
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    length += value.byteLength;
    if (length > maximum) {
      await reader.cancel();
      throw new Error("Image exceeds the cache size limit");
    }
    chunks.push(value);
  }
  const bytes = new Uint8Array(length);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return bytes;
}

export function detectImageFormat(bytes) {
  if (!(bytes instanceof Uint8Array) || bytes.length < 4) return null;
  if (matches(bytes, [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])) return "png";
  if (matches(bytes, [0xff, 0xd8, 0xff])) return "jpg";
  if (ascii(bytes, 0, 6) === "GIF87a" || ascii(bytes, 0, 6) === "GIF89a") return "gif";
  if (ascii(bytes, 0, 4) === "RIFF" && ascii(bytes, 8, 4) === "WEBP") return "webp";
  if (ascii(bytes, 4, 4) === "ftyp") {
    const brands = ascii(bytes, 8, Math.min(32, bytes.length - 8));
    if (brands.includes("avif") || brands.includes("avis")) return "avif";
  }
  if (matches(bytes, [0x42, 0x4d])) return "bmp";
  if (matches(bytes, [0x00, 0x00, 0x01, 0x00])) return "ico";
  if (matches(bytes, [0x49, 0x49, 0x2a, 0x00])
    || matches(bytes, [0x4d, 0x4d, 0x00, 0x2a])) return "tiff";
  const prefix = new TextDecoder().decode(bytes.subarray(0, Math.min(bytes.length, 1024)))
    .replace(/^\uFEFF/, "")
    .trimStart()
    .toLowerCase();
  if (prefix.startsWith("<svg") || (prefix.startsWith("<?xml") && prefix.includes("<svg"))) {
    return "svg";
  }
  return null;
}

function matches(bytes, signature) {
  return bytes.length >= signature.length
    && signature.every((value, index) => bytes[index] === value);
}

function ascii(bytes, offset, length) {
  if (bytes.length < offset + length) return "";
  return String.fromCharCode(...bytes.subarray(offset, offset + length));
}

function safeHttpUrl(value) {
  if (typeof value !== "string") return null;
  try {
    const url = new URL(value);
    return url.protocol === "http:" || url.protocol === "https:" ? url.href : null;
  } catch {
    return null;
  }
}

function clean(value, maximum) {
  if (value === null || value === undefined) return null;
  return String(value).replace(/\s+/g, " ").trim().slice(0, maximum) || null;
}
