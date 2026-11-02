#!/usr/bin/env python3
"""Regenerate differential fixtures from the sibling pinned Swift checkout."""
from pathlib import Path
import subprocess
repo = Path(__file__).resolve().parents[1]
upstream = repo.parent / "cashew"
subprocess.run(["swift", "build", "--package-path", str(upstream)], check=True)
build = Path(subprocess.check_output(["swift", "build", "--package-path", str(upstream), "--show-bin-path"], text=True).strip())
objects = sorted(str(p) for p in build.glob("*.build/*.o"))
binary = repo / "tools/fixtures"
subprocess.run(["swiftc", "-I", str(build / "Modules"), str(repo / "tools/fixtures.swift"), *objects, "-o", str(binary)], check=True)
with (repo / "tests/swift.tsv").open("w") as output:
    subprocess.run([str(binary)], stdout=output, check=True)
