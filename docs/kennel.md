# Kennel packages

Kennel is Bozzard's package store, named for Bozz. A package bundles native binaries,
Rhai scripts, typed assets and supporting files. The engine checks a package against
its own script API and asset loaders before it reaches the store, and installs it into
a project's `kennel/` folder. The public registry is the
[bozzard-plugin-library](https://github.com/Bozzard-Engine/bozzard-plugin-library)
repository. Its first package is `steam`.

```sh
cargo run -p bozzard-project -- kennel list
cargo run -p bozzard-project -- kennel info steam
cargo run -p bozzard-project -- kennel install steam path/to/my-game
cargo run -p bozzard-project -- kennel verify path/to/my-game
cargo run -p bozzard-project -- kennel remove steam path/to/my-game
```

`list [QUERY]` searches names, titles, summaries and tags. `info` prints a package's
manifest. A project is a folder containing `bozzard.project.json`, or the path to a
project manifest file.

## In the editor

Click **Kennel** in the menu bar, or check **View → Kennel · Package store**. The store opens
as a tab beside the scene view, and like any panel it can be docked elsewhere or detached.
It installs into the project that holds the active scene: the nearest folder above the
scene that contains `bozzard.project.json`.

- **Browse.** Search by words in the name, title, summary or tags, filter by category, or
  show only installed packages. Cards mark installed packages and available updates. The
  **Registry** field takes a folder or URL. Leave it empty to use `BOZZARD_KENNEL_REGISTRY`
  or the public registry.
- **Check.** A package's page compares it with this editor build: its Bozzard version
  requirement, its script API, the Cargo features it needs (the editor knows whether it
  was built with `steam`), and whether it has binaries for this machine. Install stays
  disabled until the package is compatible.
- **Install, update and remove.** These run on the store's own background job, so editing
  continues while a package downloads. **Binaries for every platform** is the same as
  `--all-targets`. When an install or removal is refused because the installed files were
  modified, the notice offers to replace or remove them anyway, as `--force` does.
  **Verify** re-hashes the whole installation.
- **Add to scene.** Adds the package's scripts and assets, and those of its dependencies,
  to the active scene's asset catalog under their declared IDs, with paths relative to the
  scene file. It is one undoable change; save the scene to keep it. Entries that already
  point into `kennel/` are updated in place, so the same button follows an upgrade.
- **Build environment.** After an install, the notice and the package page show each
  `build_env` variable as an absolute path with a Copy button.
- **Readme.** The package's `README.md` is downloaded, checked against its manifest hash,
  and rendered on the package page.

## Registries

A registry is either a folder or an HTTPS base URL that contains `index.json` and
`packages/<name>/<name>.pkg.json`. Kennel looks for one in this order:

1. The `--registry <FOLDER|URL>` flag.
2. The `BOZZARD_KENNEL_REGISTRY` environment variable.
3. The public registry's `main` branch at
   `https://raw.githubusercontent.com/Bozzard-Engine/bozzard-plugin-library/main`.

Replace `main` in that URL with a commit or a release tag (`<name>-v<version>`) to get an
install that reproduces exactly. Right after a push, GitHub's raw file server can briefly
serve an older index; Kennel reports that as a manifest checksum mismatch. Retry, or use a
URL pinned to a commit. As with content packs, plain HTTP works only on loopback.

## Installing

`install` resolves the package's dependencies and installs them first. It checks each
package's `engine` requirements before installing:

- `engine.bozzard` is a semver requirement on the engine version.
- `engine.script_api` is the lowest native script API the package's scripts need.

Files go into `<project>/kennel/<name>/`, together with an unchanged copy of the package
manifest. `<project>/kennel.lock.json` records each package's version, its registry, the
installed manifest's SHA-256 and the targets whose binaries were installed. Keep the
folder and the lockfile with the project, because scenes that use package files refer to
them.

By default, binaries are installed only for this machine's target, `<os>-<arch>` in Rust's
spelling (for example `linux-x86_64` or `macos-aarch64`). `--all-targets` installs every
platform's binaries. Installing a version that is already present does nothing. Installing
over an installation whose files changed is refused unless you pass `--force`.

The installer can't check `engine.features`, the Cargo features the engine build must
enable, so it prints them as `requires_features=`. A package's `build_env` names folders
of its binaries for a build to use. The installer prints each one as an absolute
`VARIABLE=path` line. For the `steam` package, that line is the `STEAM_SDK_LOCATION`
accepted by `tools/steam_build.rs` and `steamworks-sys`.

Scenes refer to installed scripts and assets through their own catalog, using paths
relative to the scene file. Use the package's declared IDs, because imports between a
package's scripts use them:

```json
"assets": {
  "steam/player": { "kind": "script", "path": "../kennel/steam/scripts/player.rhai" }
}
```

Export copies only the catalog assets a project uses, so binaries and documentation in
`kennel/` are never shipped by accident.

`verify` re-hashes every installed file against the installed manifest. It fails when any
of these is true:

- a file is missing or changed;
- a file is present that the package doesn't list;
- a `kennel/` folder isn't recorded in the lockfile.

`remove` refuses to remove a package that another installed package depends on. It also
refuses to remove a modified installation unless you pass `--force`.

## Package manifests

The registry's `SPEC.md` describes the format in full. A JSON Schema is published as
`schema/pkg.schema.json` for editor completion. In summary:

- `name` is a lowercase name that must match the package's folder. `version` uses semver.
- `title`, `summary`, `category` (`integration`, `scripts`, `art`, `audio`, `template` or
  `tool`), `tags`, `authors` and `license` describe the package.
- `engine`, `dependencies` and `build_env` are the requirements described above.
- `bins`, `scripts`, `assets` and `files` list every file with its `path`, `bytes` and
  `sha256`:
  - `bins` also list their `targets`. A binary can have a `source`: an HTTPS URL with its
    own SHA-256, plus, for a `tar.gz` archive, the `member` to extract.
  - `scripts` are `.rhai` or `.rs` files.
  - `assets` are typed scene assets, using the catalog's `kind` values.
  - Script and asset `id`s begin with `<name>/`.

Each file is verified along a chain of hashes. `index.json` pins each manifest's SHA-256,
the manifest pins each file, and a binary with a `source` also pins the upstream archive.
A registry stores only files without a `source`, so Valve's Steam libraries, for example,
are downloaded from the `steamworks-sys` crate rather than re-hosted. Downloads are cached
by SHA-256 in `BOZZARD_KENNEL_CACHE`, or else in a `kennel` folder beside the
[content-pack](content-packs.md) cache. A cached file is re-hashed before it is reused. The
tar reader only ever extracts the one named regular file, and never creates a path that
came from an archive.

Checksums are not signatures. They show that you received what the registry published,
not who published it. Package scripts run as game code, so install packages only from
registries you trust.

## Publishing

In a registry checkout:

1. Add or change `packages/<name>/`. When any file in a package changes, bump the
   package's `version`.
2. Run `kennel index <REGISTRY>`. It regenerates `index.json` and the copies of the
   schemas.
3. Run `kennel check <REGISTRY> --fetch`. It checks:
   - every manifest;
   - that each folder's files exactly match its manifest, with no extras;
   - that dependencies exist and satisfy their version requirements;
   - that scripts compile together with their dependencies' scripts;
   - that assets load;
   - that the index and schemas are up to date;
   - that every upstream file downloads and matches its hash (with `--fetch`).
4. Commit, then tag the release as `<name>-v<version>`.

The registry's CI runs the same check against a pinned engine revision.

## Limits

| Item | Limit |
| --- | --- |
| Index or manifest | 1 MiB |
| Packages per index | 4,096 |
| Files per package | 1,024 |
| Size of one file | 256 MiB |
| Upstream archive | 512 MiB |
| Unpacked tar scan | 1 GiB |
| Script | 1 MiB |

Kennel does not load native code at runtime. A package's binaries reach a game through the
engine's own build, for example as the `STEAM_SDK_LOCATION` for a `steam`-feature build.
`cargo test -p bozzard-project --test kennel` covers registry checks, manifest validation,
upstream downloads over a loopback server, installs, upgrades, removal and scene wiring.
`cargo test -p bozzard-editor-app kennel` drives the editor store through a dependency
install, adding to the scene with undo, verification and forced removal.
