#!/usr/bin/env bash
# Cuts a release: sets the version everywhere, commits, tags and pushes. GitHub Actions then
# builds every platform, signs the updates and publishes the release; running copies of
# OpenDictate pick it up as an update.
#
# Usage: scripts/release.sh <version>      e.g. scripts/release.sh 0.2.1
#
# Before running it, add a "## [<version>] - <date>" section to CHANGELOG.md: it becomes the
# release notes.
set -euo pipefail

cd "$(dirname "$0")/.."
VERSION=${1:-}
die() { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }
say() { printf '\033[1m==> %s\033[0m\n' "$*"; }

[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]] || die "usage: scripts/release.sh <version>, e.g. 0.2.1"
TAG="v$VERSION"
[[ "$(git rev-parse --abbrev-ref HEAD)" == main ]] || die "releases are cut from main"
git diff --quiet && git diff --cached --quiet || die "commit or stash your changes first"
git fetch --quiet origin main --tags
[[ "$(git rev-parse HEAD)" == "$(git rev-parse origin/main)" ]] || die "main is not in sync with origin/main"
git rev-parse -q --verify "refs/tags/$TAG" >/dev/null && die "$TAG already exists"
grep -q "^## \[$VERSION\]" CHANGELOG.md || die "CHANGELOG.md has no '## [$VERSION] - <date>' section"

say "setting the version to $VERSION"
npm version "$VERSION" --no-git-tag-version --allow-same-version >/dev/null
node -e '
  const fs = require("fs");
  const v = process.argv[1];
  const edit = (p, f) => fs.writeFileSync(p, f(fs.readFileSync(p, "utf8")));
  // The first `version = "…"` is the [package] version.
  edit("src-tauri/Cargo.toml", (s) => s.replace(/^version = "[^"]*"/m, `version = "${v}"`));
  edit("src-tauri/tauri.conf.json", (s) => JSON.stringify({ ...JSON.parse(s), version: v }, null, 2) + "\n");
  edit("packaging/arch/PKGBUILD", (s) => s.replace(/^pkgver=.*$/m, `pkgver=${v}`));
' "$VERSION"
(cd src-tauri && cargo update --workspace --quiet)

say "checking"
npm run build >/dev/null
(cd src-tauri && cargo test --quiet >/dev/null)

say "committing and tagging $TAG"
git add package.json package-lock.json src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json packaging/arch/PKGBUILD
git commit --quiet -m "Release $VERSION"
git tag -a "$TAG" -m "OpenDictate $VERSION"
git push --quiet origin main "$TAG"

say "pushed $TAG: https://github.com/$(gh repo view --json nameWithOwner -q .nameWithOwner 2>/dev/null || echo burakgon/opendictate-desktop)/actions"
