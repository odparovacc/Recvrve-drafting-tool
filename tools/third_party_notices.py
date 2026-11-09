#!/usr/bin/env python3
"""Writes THIRD-PARTY-NOTICES.txt: the licenses of everything Recvrve.exe contains.

    python tools/third_party_notices.py [OUTPUT] [--target TRIPLE]

Run it from the repository root after `cargo fetch` (it reads the crates' own license and
NOTICE files from Cargo's download cache). It lists the crates built into the app for the
target (Windows by default; build and test-only crates are left out), each with its license
files, identical texts printed once with every crate that uses them, then the fonts and
other material bundled in the app (from `assets/`).

For a crate offered under several licenses, the notice uses the first of PREFERRED it
offers, as the app is distributed under the GNU GPL (MIT and the like are compatible with
it). A crate that ships no license file gets the standard text of that license with the
authors from its manifest; only MIT is known for that, so anything else stops the script.

Only the standard library is used, so it runs anywhere Python 3.8+ does.
"""

import json
import os
import re
import subprocess
import sys

DEFAULT_TARGET = "x86_64-pc-windows-msvc"
PREFERRED = ["MIT", "Zlib", "BSD-3-Clause", "BSD-2-Clause", "ISC", "0BSD", "Unlicense",
             "MIT-0", "CC0-1.0", "BSL-1.0", "Apache-2.0", "MPL-2.0", "Unicode-3.0"]
LICENSE_FILE = re.compile(r"^(LICEN[CS]E|COPYING|NOTICE|UNLICENSE)", re.IGNORECASE)
# Words in a license file's name naming the license it holds.
FILE_HINTS = {
    "MIT": "MIT", "APACHE": "Apache-2.0", "ZLIB": "Zlib", "BSD": "BSD", "ISC": "ISC",
    "UNLICENSE": "Unlicense", "MPL": "MPL-2.0", "UNICODE": "Unicode-3.0", "BOOST": "BSL-1.0",
    "CC0": "CC0-1.0",
}
MIT_TEXT = """MIT License

Copyright (c) {holders}

Permission is hereby granted, free of charge, to any person obtaining a copy of this
software and associated documentation files (the "Software"), to deal in the Software
without restriction, including without limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons
to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or
substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED,
INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR
PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE
FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
"""
RULE = "=" * 88
THIN = "-" * 88


def shipped_packages(metadata):
    """The packages built into the app: the root's normal dependencies, transitively."""
    packages = {p["id"]: p for p in metadata["packages"]}
    nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}
    root = metadata["resolve"]["root"]
    seen, stack = set(), [root]
    while stack:
        current = stack.pop()
        if current in seen:
            continue
        seen.add(current)
        for dep in nodes[current]["deps"]:
            if any(kind["kind"] is None for kind in dep["dep_kinds"]):
                stack.append(dep["pkg"])
    seen.discard(root)
    return sorted((packages[i] for i in seen), key=lambda p: (p["name"], p["version"]))


def chosen(expression):
    """The license used from an SPDX expression: all of them for AND, else a preferred one."""
    expression = (expression or "").replace("/", " OR ")
    if " AND " in expression:
        return None
    options = [o.strip(" ()") for o in expression.split(" OR ")]
    for name in PREFERRED:
        if name in options:
            return name
    return options[0] if options else None


def license_files(package, license_name):
    """The package's license texts for `license_name` (all of them when None), and its
    NOTICE files (which Apache-2.0 requires to be passed on)."""
    folder = os.path.dirname(package["manifest_path"])
    names = sorted(f for f in os.listdir(folder) if LICENSE_FILE.match(f))
    if package.get("license_file"):
        extra = os.path.normpath(os.path.join(folder, package["license_file"]))
        if os.path.isfile(extra) and os.path.basename(extra) not in names:
            names.append(os.path.relpath(extra, folder))
    notices, matching, generic, other = [], [], [], []
    for name in names:
        upper = os.path.basename(name).upper()
        hints = {v for k, v in FILE_HINTS.items() if k in upper}
        if upper.startswith("NOTICE"):
            notices.append(name)
        elif not hints:
            generic.append(name)
        elif license_name is None or any(license_name.startswith(h) for h in hints):
            matching.append(name)
        else:
            other.append(name)
    # Several licenses at once (AND): every file. Otherwise the chosen license's files,
    # else whatever there is.
    picked = matching + generic + other if license_name is None else (matching or generic or other)
    texts = []
    for name in picked + notices:
        with open(os.path.join(folder, name), encoding="utf-8", errors="replace") as f:
            texts.append(f.read().strip())
    return texts


def holders(package):
    authors = [re.sub(r"\s*<[^>]*>", "", a).strip() for a in package.get("authors") or []]
    authors = [a for a in authors if a]
    return ", ".join(authors) if authors else f"the {package['name']} authors"


def crate_texts(package):
    license_name = chosen(package.get("license"))
    texts = license_files(package, license_name)
    if texts:
        return texts
    if license_name == "MIT":
        return [MIT_TEXT.format(holders=holders(package)).strip()]
    sys.exit(f"{package['name']} {package['version']}: no license file and no known "
             f"standard text for {package.get('license')!r}; add its notice by hand")


def bundled_material():
    """Fonts and other things in the app that aren't crates."""
    def read(path):
        with open(path, encoding="utf-8", errors="replace") as f:
            return f.read().strip()
    return [
        ("Lexend Deca font (assets/fonts, built into Recvrve.exe)\n"
         "Copyright 2018 The Lexend Project Authors (https://github.com/googlefonts/lexend)",
         read("assets/fonts/LexendDeca-OFL.txt")),
        ("DejaVu Sans font (assets/fonts, built into Recvrve.exe, for export text Lexend "
         "Deca lacks)", read("assets/fonts/LICENSE-DejaVu.txt")),
        ("Counter pick statistics (assets/counters.json, built into Recvrve.exe, and the\n"
         "files View > Update counter data downloads)",
         "The matchup statistics are OP.GG's (https://op.gg), read from its public MCP server\n"
         "(https://mcp-api.op.gg/mcp). Recvrve isn't affiliated with or endorsed by OP.GG."),
        ("League of Legends champion portraits and position icons",
         "Recvrve isn't endorsed by Riot Games and doesn't reflect the views or opinions of\n"
         "Riot Games or anyone officially involved in producing or managing Riot Games\n"
         "properties. Riot Games, and all associated properties (including the champion\n"
         "portraits and position icons) are trademarks or registered trademarks of Riot\n"
         "Games, Inc."),
    ]


def main():
    args = sys.argv[1:]
    target = DEFAULT_TARGET
    if "--target" in args:
        i = args.index("--target")
        target = args[i + 1]
        del args[i:i + 2]
    output = args[0] if args else "THIRD-PARTY-NOTICES.txt"
    metadata = json.loads(subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked", "--filter-platform", target],
        check=True, capture_output=True, encoding="utf-8").stdout)

    groups = {}  # identical license texts -> crates using them, in first-seen order
    for package in shipped_packages(metadata):
        key = "\n\n".join(crate_texts(package))
        line = f"{package['name']} {package['version']} ({package.get('license')})"
        if package.get("repository"):
            line += f" {package['repository']}"
        groups.setdefault(key, []).append(line)

    out = [
        "Third-party notices for Recvrve",
        "",
        "Recvrve is licensed under the GNU General Public License, version 2 or (at your",
        "option) any later version; see LICENSE. It contains the fonts and libraries below,",
        "each under its own license. A library offered under a choice of licenses is used",
        "under the most permissive one (MIT where offered), whose text is printed.",
        f"Generated from Cargo.lock by tools/third_party_notices.py for {target}.",
        "",
    ]
    for title, text in bundled_material():
        out += [RULE, title, THIN, text, ""]
    for text, crates in groups.items():
        out += [RULE, "Used by:"] + [f"  {c}" for c in crates] + [THIN, text, ""]
    with open(output, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(out).rstrip() + "\n")
    print(f"{output}: {sum(len(c) for c in groups.values())} crates, {len(groups)} texts")


if __name__ == "__main__":
    main()
