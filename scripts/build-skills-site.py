#!/usr/bin/env python3
"""Build the static site published to GitHub Pages for skill discovery.

Emits, under the output dir (default ``_site``):

    .well-known/agent-skills/index.json     # v0.2.0 discovery manifest
    .well-known/agent-skills/<name>/SKILL.md # the served artifact per skill
    index.html                               # a human landing page

The manifest follows the Agent Skills discovery schema v0.2.0 consumed by the
``npx skills`` well-known provider: each entry is a ``skill-md`` artifact whose
``digest`` is ``sha256:<hex>`` of the exact bytes served. The digest is computed
from the copied file, so it always matches and the CLI's verification passes.

Discovery then works via:  npx skills add https://<user>.github.io/<repo>/
Direct repo install stays available independently:  npx skills add <user>/<repo>

Stdlib only. Run from the repo root:  python3 scripts/build-skills-site.py
"""

from __future__ import annotations

import hashlib
import json
import shutil
import sys
from pathlib import Path

DISCOVERY_SCHEMA = "https://schemas.agentskills.io/discovery/0.2.0/schema.json"
REPO_URL = "https://github.com/boazy/xteams-cli"
WELL_KNOWN = ".well-known/agent-skills"


def split_frontmatter(text: str) -> tuple[str, str]:
    """Return (frontmatter, body). Raises if the file has no `---` block."""
    if not text.startswith("---"):
        raise ValueError("SKILL.md must start with a YAML frontmatter block")
    lines = text.splitlines()
    end = next(
        (i for i in range(1, len(lines)) if lines[i].strip() == "---"),
        None,
    )
    if end is None:
        raise ValueError("unterminated frontmatter block")
    return "\n".join(lines[1:end]), "\n".join(lines[end + 1 :])


def _unquote(value: str) -> str:
    value = value.strip()
    if len(value) >= 2 and value[0] == value[-1] and value[0] in "\"'":
        return value[1:-1]
    return value


def parse_field(frontmatter: str, key: str) -> str:
    """Extract a top-level scalar for `key`.

    Supports plain/quoted scalars and folded/literal block scalars
    (``>``, ``>-``, ``|``, ``|-``) — the subset used by our SKILL.md files.
    """
    lines = frontmatter.splitlines()
    for i, line in enumerate(lines):
        if not line.startswith(f"{key}:"):
            continue
        inline = line[len(key) + 1 :].strip()
        if inline and inline[0] in "|>":
            fold = inline[0] == ">"
            block: list[str] = []
            for cont in lines[i + 1 :]:
                if cont.strip() == "":
                    block.append("")
                    continue
                indent = len(cont) - len(cont.lstrip())
                if indent == 0:
                    break
                block.append(cont.strip())
            joined = " ".join(b for b in block if b) if fold else "\n".join(block)
            return joined.strip()
        return _unquote(inline)
    raise ValueError(f"missing required field '{key}'")


def discover_skills(skills_dir: Path) -> list[tuple[str, Path]]:
    found = []
    for child in sorted(skills_dir.iterdir()):
        skill_md = child / "SKILL.md"
        if child.is_dir() and skill_md.is_file():
            found.append((child.name, skill_md))
    return found


def build(repo_root: Path, out_dir: Path) -> dict:
    skills_dir = repo_root / "skills"
    if not skills_dir.is_dir():
        raise SystemExit(f"no skills/ directory at {skills_dir}")

    skills = discover_skills(skills_dir)
    if not skills:
        raise SystemExit(f"no skills with a SKILL.md found under {skills_dir}")

    if out_dir.exists():
        shutil.rmtree(out_dir)
    wk_dir = out_dir / WELL_KNOWN
    wk_dir.mkdir(parents=True)

    entries = []
    for dir_name, skill_md in skills:
        raw = skill_md.read_bytes()
        frontmatter, _ = split_frontmatter(raw.decode("utf-8"))
        name = parse_field(frontmatter, "name")
        description = parse_field(frontmatter, "description")
        if name != dir_name:
            raise SystemExit(
                f"skill name '{name}' must match its directory '{dir_name}'"
            )
        if len(description) > 1024:
            raise SystemExit(f"description for '{name}' exceeds 1024 chars")

        dest = wk_dir / name / "SKILL.md"
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(raw)
        digest = "sha256:" + hashlib.sha256(raw).hexdigest()

        entries.append(
            {
                "name": name,
                "type": "skill-md",
                "description": description,
                "url": f"{name}/SKILL.md",
                "digest": digest,
            }
        )

    manifest = {"$schema": DISCOVERY_SCHEMA, "skills": entries}
    (wk_dir / "index.json").write_text(
        json.dumps(manifest, indent=2) + "\n", encoding="utf-8"
    )

    (out_dir / "index.html").write_text(_landing_html(entries), encoding="utf-8")
    return manifest


def _landing_html(entries: list[dict]) -> str:
    rows = "\n".join(
        f"      <li><code>{e['name']}</code> — {e['description']}</li>"
        for e in entries
    )
    return f"""<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>xteams agent skills</title>
</head>
<body>
  <h1>xteams agent skills</h1>
  <p>Agent Skills published from
     <a href="{REPO_URL}">boazy/xteams-cli</a>.</p>
  <h2>Install</h2>
  <pre><code>npx skills add boazy/xteams-cli
npx skills add https://boazy.me/xteams-cli/</code></pre>
  <p>Discovery manifest:
     <a href="{WELL_KNOWN}/index.json">{WELL_KNOWN}/index.json</a></p>
  <h2>Skills</h2>
  <ul>
{rows}
  </ul>
</body>
</html>
"""


def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent
    out_dir = repo_root / (sys.argv[1] if len(sys.argv) > 1 else "_site")
    manifest = build(repo_root, out_dir)
    names = ", ".join(s["name"] for s in manifest["skills"])
    print(f"built {out_dir} with {len(manifest['skills'])} skill(s): {names}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
