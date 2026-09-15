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
# Reviewed Rapier/Parry CPU physics, math, collections and derive dependencies. No importers or presentation.
allowed |= {"rapier3d", "parry3d", "nalgebra", "nalgebra-macros", "glamx", "simba", "approx",
            "num-traits", "num-complex", "num-integer", "num-rational", "num-derive", "typenum",
            "matrixmultiply", "rawpointer", "libm", "wide", "safe_arch", "bytemuck", "arrayvec",
            "smallvec", "bitflags", "byteorder", "downcast-rs", "either", "ena", "equivalent",
            "foldhash", "hash32", "hashbrown", "heapless", "indexmap", "log", "ordered-float",
            "profiling", "profiling-procmacros", "rstar", "slab", "stable_deref_trait",
            "static_assertions", "thiserror", "thiserror-impl"}
# Rhai scripting: a pure-Rust interpreter plus its hashing and string dependencies. No graphics,
# window or audio crates, so a dedicated server still runs scripts headlessly.
allowed |= {"rhai", "rhai_codegen", "ahash", "once_cell", "smartstring", "thin-vec",
            "cfg-if", "crunchy", "const-random", "const-random-macro", "getrandom", "libc",
            "portable-atomic", "tiny-keccak", "zerocopy"}
# Reviewed CPU text shaping/metrics shared by authored UI layout and rendering. Epaint is
# built without its window/GPU integration; no egui, winit, wgpu, importers, or audio devices.
allowed |= {"bozzard-text", "epaint", "epaint_default_fonts", "ecolor", "emath", "ahash",
            "bytemuck_derive", "cfg-if", "color", "euclid", "fearless_simd", "font-types",
            "guillotiere", "harfrust", "kurbo", "libc", "linebender_resource_handle",
            "lock_api", "nohash-hasher", "once_cell", "parking_lot", "parking_lot_core",
            "peniko", "polycool", "read-fonts", "scopeguard", "self_cell", "skrifa",
            "unicode-general-category", "unicode-segmentation", "vello_common", "vello_cpu",
            "zerocopy", "windows-link"}
unexpected = {line.split()[0] for line in output.splitlines() if line.strip()} - allowed
if unexpected:
    raise SystemExit(f"Headless dependency boundary changed: {sorted(unexpected)}. Review before extending the allowlist.")
print("headless_dependencies_ok: simulation, CPU physics/text metrics, math, serialization; no device, window or importer crates")
