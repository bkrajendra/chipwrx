#!/usr/bin/env node
// Renders `public/index.html` for GitHub Pages (`.github/workflows/pages.yml`, M10/NFR-D1):
// a single static page listing the latest GitHub Release's per-platform download links.
// Run in CI right after a release is published; safe to run locally too (`node
// scripts/render-download-page.mjs > public/index.html`) against whatever release is
// currently "latest" on GitHub.

const REPO = process.env.GITHUB_REPOSITORY || "bkrajendra/chipwrx";

function escapeHtml(s) {
  return s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
}

function humanSize(bytes) {
  const mb = bytes / (1024 * 1024);
  return `${mb.toFixed(1)} MB`;
}

/** Buckets a release asset by platform from its filename — the bundler names are stable
 * across Tauri versions (`.dmg` macOS, `.msi`/`.exe` Windows, `.AppImage`/`.deb` Linux) —
 * and drops anything else (`.sig`, `latest.json`, checksums) that isn't a real installer. */
function platformFor(name) {
  if (name.endsWith(".dmg")) return "macOS";
  if (name.endsWith(".msi") || name.endsWith(".exe")) return "Windows";
  if (name.endsWith(".AppImage") || name.endsWith(".deb")) return "Linux";
  return null;
}

async function fetchLatestRelease() {
  const res = await fetch(`https://api.github.com/repos/${REPO}/releases/latest`, {
    headers: { Accept: "application/vnd.github+json", "User-Agent": "vibe-hardware-pages" },
  });
  if (!res.ok) {
    throw new Error(`GitHub API ${res.status} fetching latest release for ${REPO}: ${await res.text()}`);
  }
  return res.json();
}

function render(release) {
  const groups = { macOS: [], Windows: [], Linux: [] };
  for (const asset of release.assets ?? []) {
    const platform = platformFor(asset.name);
    if (!platform) continue;
    groups[platform].push(asset);
  }

  const card = (platform, icon) => {
    const assets = groups[platform];
    if (assets.length === 0) {
      return `<section class="card"><h2>${icon} ${platform}</h2><p class="muted">No ${platform} build in this release yet.</p></section>`;
    }
    const links = assets
      .map((a) => `<a class="asset" href="${a.browser_download_url}">${escapeHtml(a.name)} <span class="muted">(${humanSize(a.size)})</span></a>`)
      .join("\n        ");
    return `<section class="card">
        <h2>${icon} ${platform}</h2>
        ${links}
      </section>`;
  };

  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>Vibe Hardware — Download</title>
<style>
  :root { color-scheme: light dark; }
  body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; max-width: 640px; margin: 4rem auto; padding: 0 1.5rem; line-height: 1.5; }
  h1 { font-size: 1.5rem; }
  .version { color: #888; font-size: 0.9rem; margin-bottom: 2rem; }
  .grid { display: grid; gap: 1rem; }
  .card { border: 1px solid #8883; border-radius: 8px; padding: 1rem 1.25rem; }
  .card h2 { font-size: 1rem; margin: 0 0 0.5rem; }
  .asset { display: block; padding: 0.4rem 0; text-decoration: none; color: #2563eb; }
  .asset:hover { text-decoration: underline; }
  .muted { color: #888; font-size: 0.85rem; }
  footer { margin-top: 3rem; font-size: 0.85rem; color: #888; }
  a { color: #2563eb; }
</style>
</head>
<body>
  <h1>Vibe Hardware</h1>
  <p class="version">Latest release: <strong>${escapeHtml(release.tag_name)}</strong> — published ${new Date(release.published_at).toLocaleDateString("en-US", { year: "numeric", month: "long", day: "numeric" })}</p>
  <div class="grid">
    ${card("macOS", "🍎")}
    ${card("Windows", "🪟")}
    ${card("Linux", "🐧")}
  </div>
  <footer>
    <p><a href="${release.html_url}">Release notes</a> · <a href="https://github.com/${REPO}">Source on GitHub</a></p>
  </footer>
</body>
</html>
`;
}

const release = await fetchLatestRelease();
process.stdout.write(render(release));
