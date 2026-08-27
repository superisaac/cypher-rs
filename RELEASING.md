# Publishing to crates.io

The crates.io package is `cypher-rs-zk`. Its library target is
`cypher_rs_zk`, following Cargo's standard hyphen-to-underscore mapping.

## Prerequisites

- Confirm the intended version in `Cargo.toml` and move release notes from
  `Unreleased` to that version in `CHANGELOG.md`.
- Confirm the repository's `main` branch is current and CI passes.
- Authenticate with `cargo login` or provide a scoped crates.io token through
  `CARGO_REGISTRY_TOKEN`. Do not commit the token.
- For the first release, confirm `https://crates.io/crates/cypher-rs-zk` is
  still unclaimed and that the authenticated account should own it.

## Validate

Run these checks from a clean checkout:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo +1.74 test --all-features
cargo package --list
cargo publish --dry-run --all-features
```

The package list must contain the Rust sources, tests, example, README,
changelog, documentation, and license. It must not contain `vendors/`, local
agent instructions, CI configuration, or build output.

`cargo publish --dry-run --all-features` builds the generated archive, which catches files
that were available in the checkout but omitted from the published package.
The resulting archive is written under `target/package/` and can be inspected
with:

```bash
tar -tzf target/package/cypher-rs-zk-1.1.0.crate
```

## Publish

Commit the final metadata and documentation, then tag that exact commit using
the package-qualified tag name. For version 1.1.0:

```bash
git tag -s cypher-rs-zk-v1.1.0
git push origin main cypher-rs-zk-v1.1.0
cargo publish
```

Publishing is permanent: crates.io does not allow replacing a released
version. Run `cargo publish` only after the dry run succeeds on the tagged,
clean commit.

After publication, verify the package page and generated API documentation:

- `https://crates.io/crates/cypher-rs-zk`
- `https://docs.rs/cypher-rs-zk/1.1.0/cypher_rs_zk/`

Finally, create the matching GitHub release from the package-qualified tag.
