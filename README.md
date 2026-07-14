# Livreur

> livreur is a solution to easily distribute rust packages

## Features

Livreur allows you to release your rust package as:

- binaries
- an npm package
- a Homebrew tap
- a Windows install

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

## Build

Build and upload one native target from each GitHub Actions matrix job:

```console
livreur build --target x86_64-unknown-linux-gnu
livreur build --target aarch64-apple-darwin --tag v1.2.3
```

The tag comes from `--tag` or `GITHUB_REF_NAME`. Livreur runs Cargo in release
mode, creates a flat `.tar.gz` (or `.zip` on Windows) containing the binary and
any `README*` and `LICENSE*` files, then uploads it with `--clobber`. The tag must
already exist. The first build creates a draft GitHub release; later matrix jobs
reuse it. Build refuses to replace assets after a release is published because
doing so would make its `SHA256SUMS` stale. A narrow draft-creation race remains
if multiple first-time matrix jobs reach GitHub simultaneously, so publishing
always checks the complete asset set.

Livreur owns Cargo's `--release`, `--target`, `--message-format`,
`--manifest-path`, and `--bin` arguments. Do not repeat or override them in
`build.cargo_args`.

## Publish

After every target build succeeds, verify the release and publish it:

```console
livreur publish
livreur publish --tag v1.2.3 --format json
```

Publish refuses an incomplete asset set. It downloads the uploaded archives,
computes one sorted, `sha256sum -c`-compatible `SHA256SUMS`, uploads it with
`--clobber`, and removes draft status. Re-running against an already published
release is a successful read-only no-op.

### Release description templates

Livreur fills the GitHub release description from an embedded Markdown
[Tera](https://keats.github.io/tera/docs/) template when publishing. Extract a
copy and add it to `livreur.toml` with:

```console
livreur template release
```

This creates `.github/release.md.tera` and configures it as
`release.template`. Existing files or configuration are preserved unless
`--force` is passed. Use `--output` and `--config` to choose other paths;
relative output paths are resolved from the configuration file's directory.

Templates receive the following stable values:

- `package`: `name`, `version`, `description`, `license`, `repository`,
  `authors`, and `binary`.
- `release`: `tag`, `version`, `url`, and `checksums.name`/`checksums.url`.
- `platforms`: an ordered list with `target`, `architecture`, `os`, and
  `asset.name`/`asset.url`/`asset.sha256` for each configured target.
- `channels`: `installers`, `npm`, `homebrew`, and `crates` configuration.

For example:

```tera
{% for platform in platforms %}
- [{{ platform.target }}]({{ platform.asset.url }}) (`{{ platform.asset.sha256 }}`)
{% endfor %}
```

`livreur validate` reads and renders configured templates with representative
release data, so invalid syntax and unknown variables fail before publishing.

Both commands shell out to the GitHub `gh` CLI in the current repository. Set
`GH_TOKEN` for authentication; no owner/repository setting is required.

## GitHub Actions

`livreur init` creates both `livreur.toml` and
`.github/workflows/release.yml`. Use `--no-workflow` to create only the config,
or `--workflow <path>` to choose another workflow location. The generated
workflow uses native Linux, macOS, and Windows matrix runners, followed by one
publish job.

The workflow needs `permissions: contents: write` and exposes
`${{ github.token }}` as `GH_TOKEN`. Keep its matrix synchronized with
`release.targets`, and adjust its tag glob if you change `release.tag`.

Generated workflows install Livreur with `getlivreur/setup-livreur@v1`. By
default the action selects the latest stable release. Pin a stable version in
`livreur.toml` when rendering the workflow (a leading `v` is accepted and
normalized away):

```toml
[tool]
version = "1.2.3"
```

Use `version = "source"` to build Livreur's latest `main` commit instead. The
generated workflow then installs the stable Rust toolchain before running the
setup action. Tool selection is embedded when the workflow is generated, so
regenerate the workflow after changing this setting.
