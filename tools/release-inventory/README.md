# Reviewed browser release inventory guard

This private Rust build tool binds full notices to the crate version, release
tag, WASM feature profile, compiler/wasm-pack versions, source files, Cargo lock,
and notice digests. It refuses drift before writing npm package files.

Run it from the repository root:

```sh
cargo run --locked --manifest-path tools/release-inventory/Cargo.toml -- \
  verify . v0.2.0 "$(rustc --version)" "$(wasm-pack --version)"
```

After wasm-pack writes `pkg`, use `package` with the same arguments. It copies
full notices and source URLs into `pkg` and adds them to its explicit npm files
allowlist. `npm pack --dry-run --json` must confirm both files are included.

When release inputs change, audit the new dependency closure and sysroot,
review the full notices and source inventory, then update
`docs/release-inventory.json` with those reviewed inputs and SHA-256 digests.
Do not update hashes merely to bypass a failed review. Existing frozen assets
and their release companions stay tied to their original source and digest.

This tool uses RustCrypto SHA-256; it is not part of the browser runtime.
