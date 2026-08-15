import { opendir, readFile, realpath, stat } from "node:fs/promises";
import { basename, extname, relative, resolve, sep } from "node:path";

const DENIED_SEGMENTS = new Set([
  ".git", ".hg", ".svn", ".ssh", ".aws", ".config", ".cache", ".codex",
  "node_modules", "target", "dist", "build", "coverage", "vendor", "__pycache__",
  ".venv", "venv",
]);
const DENIED_NAMES = new Set([
  ".env", ".npmrc", ".pypirc", "credentials", "credentials.json", "secrets.json",
]);
const DENIED_EXTENSIONS = new Set([".key", ".pem", ".p12", ".pfx", ".jks", ".keystore"]);
const TEXT_EXTENSIONS = new Set([
  ".c", ".cc", ".cpp", ".cs", ".css", ".csv", ".go", ".graphql", ".h", ".hpp",
  ".html", ".java", ".js", ".jsx", ".json", ".kt", ".kts", ".md", ".mjs", ".py",
  ".rb", ".rs", ".scss", ".sh", ".sql", ".svelte", ".swift", ".toml", ".ts",
  ".tsx", ".txt", ".vue", ".xml", ".yaml", ".yml", ".zig",
]);
const TEXT_NAMES = new Set([
  "cargo.lock", "cargo.toml", "dockerfile", "justfile", "license", "makefile", "readme",
]);

export async function listProjectFiles(root, options = {}) {
  const projectRoot = await realpath(root);
  const maximum = clamp(options.limit, 1, 800, 300);
  const depthLimit = clamp(options.maxDepth, 1, 12, 8);
  const prefix = await safeDirectory(projectRoot, options.path ?? ".");
  const files = [];
  let truncated = false;

  async function walk(directory, depth) {
    if (files.length >= maximum) {
      truncated = true;
      return;
    }
    const entries = [];
    for await (const entry of await opendir(directory)) entries.push(entry);
    entries.sort((left, right) => left.name.localeCompare(right.name));
    for (const entry of entries) {
      const absolute = resolve(directory, entry.name);
      const projectPath = relative(projectRoot, absolute);
      if (isDenied(projectPath) || entry.isSymbolicLink()) continue;
      if (entry.isDirectory()) {
        if (depth < depthLimit) await walk(absolute, depth + 1);
      } else if (entry.isFile() && isTextPath(projectPath)) {
        files.push(projectPath);
        if (files.length >= maximum) {
          truncated = true;
          break;
        }
      }
    }
  }

  await walk(prefix, 0);
  return { root: projectRoot, path: relative(projectRoot, prefix) || ".", files, truncated };
}

export async function searchProject(root, query, options = {}) {
  const needle = String(query ?? "").trim();
  if (!needle) throw new Error("Search query must not be empty");
  if (needle.length > 240) throw new Error("Search query is too long");
  const projectRoot = await realpath(root);
  const fileLimit = clamp(options.fileLimit, 1, 1_500, 700);
  const matchLimit = clamp(options.limit, 1, 120, 60);
  const listing = await listProjectFiles(projectRoot, {
    path: options.path ?? ".",
    limit: fileLimit,
    maxDepth: 12,
  });
  const normalizedNeedle = options.caseSensitive ? needle : needle.toLocaleLowerCase();
  const matches = [];
  for (const projectPath of listing.files) {
    const absolute = await safeFile(projectRoot, projectPath);
    const metadata = await stat(absolute);
    if (metadata.size > 1_500_000) continue;
    const contents = await readFile(absolute, "utf8");
    if (contents.includes("\0")) continue;
    for (const [index, line] of contents.split(/\r?\n/).entries()) {
      const candidate = options.caseSensitive ? line : line.toLocaleLowerCase();
      if (!candidate.includes(normalizedNeedle)) continue;
      matches.push({ path: projectPath, line: index + 1, text: truncate(line.trim(), 280) });
      if (matches.length >= matchLimit) {
        return { query: needle, matches, truncated: true };
      }
    }
  }
  return { query: needle, matches, truncated: listing.truncated };
}

export async function readProjectFile(root, requestedPath, options = {}) {
  const projectRoot = await realpath(root);
  const absolute = await safeFile(projectRoot, requestedPath);
  const metadata = await stat(absolute);
  if (metadata.size > 2_000_000) throw new Error("File is too large to read safely");
  const contents = await readFile(absolute, "utf8");
  if (contents.includes("\0")) throw new Error("Binary files are not readable");
  const lines = contents.split(/\r?\n/);
  const start = clamp(options.startLine, 1, Math.max(1, lines.length), 1);
  const end = clamp(options.endLine, start, Math.min(lines.length, start + 399), Math.min(lines.length, start + 199));
  const selected = lines.slice(start - 1, end).map((line, index) => `${start + index}: ${line}`).join("\n");
  return {
    path: relative(projectRoot, absolute),
    start_line: start,
    end_line: end,
    total_lines: lines.length,
    content: truncate(selected, 60_000),
  };
}

async function safeDirectory(projectRoot, requestedPath) {
  const absolute = await safePath(projectRoot, requestedPath);
  const metadata = await stat(absolute);
  if (!metadata.isDirectory()) throw new Error("Requested path is not a directory");
  return absolute;
}

async function safeFile(projectRoot, requestedPath) {
  const absolute = await safePath(projectRoot, requestedPath);
  const projectPath = relative(projectRoot, absolute);
  if (!isTextPath(projectPath)) throw new Error("Only source and text files are readable");
  const metadata = await stat(absolute);
  if (!metadata.isFile()) throw new Error("Requested path is not a file");
  return absolute;
}

async function safePath(projectRoot, requestedPath) {
  const candidate = String(requestedPath ?? ".");
  if (candidate.includes("\0") || isDenied(candidate)) throw new Error("Path is not readable");
  const lexical = resolve(projectRoot, candidate);
  if (!inside(projectRoot, lexical)) throw new Error("Path escapes the project root");
  const canonical = await realpath(lexical);
  if (!inside(projectRoot, canonical)) throw new Error("Path escapes the project root");
  if (isDenied(relative(projectRoot, canonical))) throw new Error("Path is not readable");
  return canonical;
}

function inside(root, candidate) {
  const path = relative(root, candidate);
  return path === "" || (!path.startsWith(`..${sep}`) && path !== "..");
}

function isDenied(projectPath) {
  const segments = String(projectPath).replaceAll("\\", "/").split("/").filter(Boolean);
  return segments.some((segment) => {
    const lower = segment.toLocaleLowerCase();
    return DENIED_SEGMENTS.has(lower)
      || DENIED_NAMES.has(lower)
      || lower.startsWith(".env.")
      || lower.includes("secret")
      || lower.includes("credential")
      || DENIED_EXTENSIONS.has(extname(lower));
  });
}

function isTextPath(projectPath) {
  if (isDenied(projectPath)) return false;
  const name = basename(projectPath).toLocaleLowerCase();
  return TEXT_EXTENSIONS.has(extname(name)) || TEXT_NAMES.has(name) || name.startsWith("readme.");
}

function clamp(value, minimum, maximum, fallback) {
  const parsed = Math.round(Number(value));
  return Number.isFinite(parsed) ? Math.max(minimum, Math.min(maximum, parsed)) : fallback;
}

function truncate(value, maximum) {
  const text = String(value ?? "");
  return text.length <= maximum ? text : `${text.slice(0, maximum - 1)}…`;
}
