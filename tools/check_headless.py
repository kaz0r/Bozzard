#!/usr/bin/env python3
"""Audit the headless dependency allowlist; math/serialization are allowed, presentation is not."""
import subprocess

output = subprocess.check_output(
    ["cargo", "tree", "--locked", "--offline", "-p", "bozzard-server", "--edges", "normal", "--prefix", "none", "--format", "{p}"],
    text=True,
)
allowed = {"bozzard-server", "bozzard-demo", "bozzard-app", "bozzard-ecs", "bozzard-scene",
           "anyhow", "glam", "serde", "serde_core", "serde_derive", "serde_json",
           "itoa", "memchr", "zmij", "proc-macro2", "quote", "syn", "unicode-ident"}
unexpected = {line.split()[0] for line in output.splitlines() if line.strip()} - allowed
if unexpected:
    raise SystemExit(f"Headless dependency boundary changed: {sorted(unexpected)}. Review before extending the allowlist.")
print("headless_dependencies_ok: simulation, math, serialization; no graphics or window crates")
