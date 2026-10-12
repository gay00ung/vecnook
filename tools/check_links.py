#!/usr/bin/env python3
"""Check public Markdown relative files and heading anchors without a network crawl."""
from pathlib import Path
import re
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]


def anchors(path):
    counts, result = {}, set()
    for heading in re.findall(r"(?m)^#{1,6} (.+?)\s*#*\s*$", path.read_text(encoding="utf-8")):
        plain = re.sub(r"\[([^]]+)\]\([^)]*\)", r"\1", heading)
        key = re.sub(r"[^\w\- ]", "", plain.lower()).replace(" ", "-")
        number = counts.get(key, 0)
        result.add(key if not number else f"{key}-{number}")
        counts[key] = number+1
    return result


def main():
    paths = [ROOT / p for p in ["README.md", "CHANGELOG.md", "CONTRIBUTING.md"]]
    paths += sorted((ROOT / "docs").glob("*.md"))+sorted((ROOT / "examples").rglob("*.md"))
    errors, checked = [], 0
    for path in paths:
        for _, url in re.findall(r"!?\[([^]]*)\]\(([^)]+)\)", path.read_text(encoding="utf-8")):
            url = url.strip("<>")
            parsed = urlsplit(url)
            if parsed.scheme or parsed.netloc:
                continue
            target = (path.parent / unquote(parsed.path)).resolve() if parsed.path else path
            checked += 1
            if not target.is_file():
                errors.append(f"{path.relative_to(ROOT)}: missing {url}")
            elif parsed.fragment and target.suffix == ".md" and unquote(parsed.fragment) not in anchors(target):
                errors.append(f"{path.relative_to(ROOT)}: missing anchor {url}")
    if errors:
        raise ValueError("\n".join(errors))
    print(f"Checked {checked} relative links in {len(paths)} public Markdown documents")


if __name__ == "__main__":
    main()
