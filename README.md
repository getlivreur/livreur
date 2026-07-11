# Livreur

> livreur is a solution to easily distribute rust packages

## Features

Livreur allows you to release your rust package as:

- binaries
- an npm package
- a Homebrew tap
- a Windows install

Initialize a Cargo package with an interactive GitHub release setup:

```console
livreur init
livreur init --yes
```

This creates `livreur.toml` and a fully managed
`.github/workflows/release.yml`. Existing files are never replaced without an
interactive confirmation or `--force`.

## Validate configuration

Livreur reads release policy from a versioned `livreur.toml`. Validate it together
with the selected Cargo package before generating or running a release:

```console
livreur validate
livreur validate --tag v1.2.3
livreur validate --format json
```

Validation is local and side-effect-free. It resolves Cargo metadata, defaults,
targets, and publication channels; it does not contact providers or change files.
Use `--config` and `--manifest-path` when the files are not in the current
directory.

Unknown TOML fields and unsupported schema versions are rejected. The release
version is parsed from the supplied tag and must match Cargo's package version.
Prerelease and build metadata versions are not supported yet.
