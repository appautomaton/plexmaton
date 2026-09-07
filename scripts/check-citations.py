"""Validate the corpus's invariant ownership, source references, and local Markdown links."""

from collections import defaultdict
from pathlib import Path
import re
import sys
from urllib.parse import unquote, urlsplit


ROOT = Path(__file__).resolve().parents[1]
INVARIANT = re.compile(r"\b[A-Z]{2,6}-[0-9]+\b")
DECLARATION = re.compile(r"^\*\*(([A-Z]{2,6})-[0-9]+) ", re.MULTILINE)
SECTION = re.compile(r"ui-ux(?:\.md)?`? §([a-z][a-z -]*[a-z])[).,;:]")


def prose(source):
    """Keep line positions while excluding fenced examples from document declarations."""
    fence = None
    lines = []
    for line in source.splitlines():
        marker = re.match(r"^ {0,3}(`{3,}|~{3,})(.*)$", line)
        if fence:
            if (marker and marker[1][0] == fence[0] and len(marker[1]) >= len(fence)
                    and not marker[2].strip()):
                fence = None
            lines.append("")
        elif marker:
            fence = marker[1]
            lines.append("")
        else:
            lines.append(line)
    return "\n".join(lines)


def headings(source):
    lines = prose(source).splitlines()
    for index, line in enumerate(lines):
        atx = re.match(r"^ {0,3}#{1,6}\s+(.+?)\s*$", line)
        if atx:
            yield re.sub(r"\s+#+\s*$", "", atx[1])
        elif (index and lines[index - 1].strip()
              and re.fullmatch(r" {0,3}(?:=+|-+)\s*", line)):
            yield lines[index - 1].strip()


def heading_anchors(source):
    anchors = set()
    for heading in headings(source):
        # Headings in this corpus use text, inline formatting, and inline links.
        heading = re.sub(r"!?\[([^\]]*)\]\([^)]*\)", r"\1", heading)
        heading = re.sub(r"<[^>]*>", "", heading)
        heading = re.sub(r"(?<!\w)_([^_]+)_(?!\w)", r"\1", heading)
        base = re.sub(r"[^\w\- ]", "", heading.lower()).replace(" ", "-")
        anchor = base
        suffix = 0
        while anchor in anchors:
            suffix += 1
            anchor = f"{base}-{suffix}"
        anchors.add(anchor)
    return anchors


def check(root):
    errors = []
    for required in ("crates", ".agents/specs"):
        if not (root / required).is_dir():
            errors.append(f"citation: missing source directory {required}")
    if errors:
        return errors, ""

    rust = {path: path.read_text() for path in sorted((root / "crates").rglob("*.rs"))}
    documents = {path: path.read_text() for path in sorted((root / ".agents").rglob("*.md"))}
    for name in ("AGENTS.md", "README.md"):
        path = root / name
        if path.is_file():
            documents[path] = path.read_text()

    declarations = defaultdict(list)
    prefixes = defaultdict(set)
    proofs = set()
    for path, source in documents.items():
        if path.parent != root / ".agents/specs":
            continue
        body = prose(source)
        for match in DECLARATION.finditer(body):
            line = body.count("\n", 0, match.start()) + 1
            declarations[match[1]].append(f"{path.relative_to(root)}:{line}")
            prefixes[match[2]].add(str(path.relative_to(root)))
        in_evidence = False
        for line in body.splitlines():
            if line.startswith("## Evidence"):
                in_evidence = True
            elif line.startswith("## "):
                in_evidence = False
            elif in_evidence and re.match(r"^\| [A-Z]+-[0-9]+ \|", line):
                proofs.update(re.findall(r"`([a-z][a-z0-9_]+)`", line))

    for identity, locations in sorted(declarations.items()):
        if len(locations) > 1:
            errors.append(f"citation: duplicate invariant {identity}: {', '.join(locations)}")
    for prefix, owners in sorted(prefixes.items()):
        if len(owners) > 1:
            errors.append(f"citation: prefix {prefix} belongs to multiple specs: {', '.join(sorted(owners))}")

    cited = {identity for source in rust.values() for identity in INVARIANT.findall(source)
             if identity.split("-", 1)[0] not in {"UTF", "SHA", "RGB", "ISO", "HTTP"}}
    for identity in sorted(cited - declarations.keys()):
        errors.append(f"citation: {identity} is cited in code but declared in no spec")
    functions = {name for source in rust.values()
                 for name in re.findall(r"\bfn ([a-z][a-z0-9_]+)\(", source)}
    for name in sorted(proofs - functions):
        errors.append(f"evidence: {name} is named as proof but no such source function exists")

    sections = {section for source in [*rust.values(), *documents.values()]
                for section in SECTION.findall(source)}
    contract = documents.get(root / ".agents/ui-ux.md", "")
    contract_headings = [heading.lower() for heading in headings(contract)]
    for section in sorted(sections):
        if not any(section in heading for heading in contract_headings):
            errors.append(f"citation: ui-ux §{section} is cited but no such heading exists")

    anchors = {}
    for path, source in documents.items():
        # The corpus uses inline Markdown links. External URLs are not fetched.
        for match in re.finditer(r"\]\((<[^>\n]+>|[^)\n]+)\)", prose(source)):
            raw = match[1].strip()
            if not raw:
                continue
            link = raw[1:-1] if raw.startswith("<") else raw.split()[0]
            parts = urlsplit(link)
            if parts.scheme or parts.netloc:
                continue
            target = (path.parent / unquote(parts.path)).resolve() if parts.path else path
            if not target.exists():
                errors.append(f"link: {path.relative_to(root)} points at {link}, which does not exist")
            elif parts.fragment and target.is_file() and target.suffix == ".md":
                if target not in anchors:
                    anchors[target] = heading_anchors(target.read_text())
                if unquote(parts.fragment) not in anchors[target]:
                    errors.append(f"link: {path.relative_to(root)} points at missing heading {link}")

    summary = (f"citations: {len(cited)} invariants and {len(sections)} contract sections cited, "
               f"{len(proofs)} named proofs; all resolve with unique owners")
    return errors, summary


if __name__ == "__main__":
    failures, summary = check(ROOT)
    if failures:
        print("\n".join(failures), file=sys.stderr)
        sys.exit(1)
    print(summary)
