#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Build the StormSewer manual: docs/*.md -> a static HTML site with pandoc.
#
#   build-tools/build-docs.sh [OUT_DIR]        default OUT_DIR = site/manual
#
# Needs pandoc on PATH (apt install pandoc; on Windows %LOCALAPPDATA%\Pandoc
# is also tried). No npm, no Rust, no templates beyond the CSS beside this
# script. The sidebar table of contents is generated from the file list, so a
# new chapter needs nothing but its file: the order is index first, then the
# numbered chapters, then the A-appendices.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/.." && pwd)"
docs="$root/docs"
out="${1:-$root/site/manual}"

pandoc_bin="${PANDOC:-}"
if [ -z "$pandoc_bin" ]; then
  if command -v pandoc >/dev/null 2>&1; then
    pandoc_bin=pandoc
  elif [ -n "${LOCALAPPDATA:-}" ] && [ -x "$LOCALAPPDATA/Pandoc/pandoc.exe" ]; then
    pandoc_bin="$LOCALAPPDATA/Pandoc/pandoc.exe"
  else
    echo "build-docs: pandoc not found (install it, or set PANDOC=/path/to/pandoc)" >&2
    exit 1
  fi
fi

mkdir -p "$out/img" "$out/datasets"
cp "$here/docs.css" "$out/manual.css"
[ -d "$docs/img" ] && cp -r "$docs/img/." "$out/img/"
[ -d "$docs/datasets" ] && cp -r "$docs/datasets/." "$out/datasets/"

# Chapter order: index, 01.., A1.. — the file names carry it.
# Chapter files: two digits, optionally a letter for a companion chapter
# (20b follows 20). C collation keeps that order on every machine.
mapfile -t files < <(cd "$docs" && LC_ALL=C ls index.md [0-9][0-9]-*.md [0-9][0-9][a-z]-*.md A[0-9]-*.md 2>/dev/null)

# Title of a chapter = its first "# " heading.
title_of() {
  sed -n 's/^# //p' "$docs/$1" | head -n 1
}

# The sidebar, once, as an HTML fragment every page includes.
nav="$(mktemp)"
{
  echo '<nav class="sidebar"><div class="brand"><a href="index.html">StormSewer manual</a></div><ol>'
  for f in "${files[@]}"; do
    page="${f%.md}.html"
    t="$(title_of "$f")"
  # Browser-tab title only: giving pandoc a "title" as well prints a second
  # heading above the chapter's own "# " line. The index page is the
  # manual, so its tab is not "StormSewer manual — StormSewer manual".
  if [ "$f" = "index.md" ]; then pt="$t"; else pt="$t — StormSewer manual"; fi
    [ "$f" = "index.md" ] && t="Contents"
    printf '<li><a href="%s">%s</a></li>\n' "$page" "$t"
  done
  echo '</ol><div class="foot"><a href="../">Web demo</a> · <a href="https://github.com/mf4633/stormsewer">Source</a></div></nav>'
} > "$nav"

version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$root/Cargo.toml" | head -n 1)"

for f in "${files[@]}"; do
  page="${f%.md}.html"
  t="$(title_of "$f")"
  # Browser-tab title only: giving pandoc a "title" as well prints a second
  # heading above the chapter's own "# " line. The index page is the
  # manual, so its tab is not "StormSewer manual — StormSewer manual".
  if [ "$f" = "index.md" ]; then pt="$t"; else pt="$t — StormSewer manual"; fi
  # Markdown links between chapters point at .md; the site is .html.
  sed -E 's/\]\(([0-9A-Za-z_-]+)\.md(#[^)]*)?\)/](\1.html\2)/g' "$docs/$f" \
    | "$pandoc_bin" \
        --from gfm+tex_math_dollars \
        --to html5 \
        --standalone \
        --toc --toc-depth=2 \
        --css manual.css \
        --metadata "pagetitle=$pt" \
        --variable "include-before=$(cat "$nav")" \
        --variable "header-includes=<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">" \
        --variable "include-after=<footer>StormSewer $version · GPL-3.0-or-later · built with pandoc</footer>" \
        --output "$out/$page"
done
rm -f "$nav"

# .nojekyll so GitHub Pages serves files that begin with an underscore.
touch "$out/.nojekyll"

n=${#files[@]}
size=$(du -sh "$out" 2>/dev/null | cut -f1)
echo "build-docs: $n pages -> $out ($size)"
