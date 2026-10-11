//! Private release guard: refuses stale notices and includes them in npm packages.
//!
//! Run `verify ROOT TAG RUSTC_VERSION WASM_PACK_VERSION` before building, then
//! `package` with the same arguments after wasm-pack writes `ROOT/pkg`.
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Inventory {
    version: String,
    source_commit: String,
    target: String,
    features: Vec<String>,
    rustc_version: String,
    wasm_pack_version: String,
    source_files: BTreeMap<String, String>,
    notice_files: BTreeMap<String, String>,
}

fn hash(path: &Path) -> Result<String> {
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(format!("refusing symlink {}", path.display()).into());
    }
    Ok(format!("{:x}", Sha256::digest(fs::read(path)?)))
}

fn collect(root: &Path, path: &Path, files: &mut BTreeMap<String, String>) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(format!("refusing source symlink {}", path.display()).into());
    }
    if meta.is_dir() {
        for entry in fs::read_dir(path)? {
            collect(root, &entry?.path(), files)?;
        }
    } else if meta.is_file() {
        let key = path
            .strip_prefix(root)?
            .to_str()
            .ok_or("non-UTF-8 source path")?;
        files.insert(key.replace('\\', "/"), hash(path)?);
    } else {
        return Err("unsupported source file type".into());
    }
    Ok(())
}

fn source_files(root: &Path) -> Result<BTreeMap<String, String>> {
    let mut files = BTreeMap::new();
    for name in [
        "Cargo.toml",
        "Cargo.lock",
        "src",
        "tests",
        "examples",
        "benches",
        ".cargo",
        "build.rs",
    ] {
        let path = root.join(name);
        if path.exists() {
            collect(root, &path, &mut files)?;
        }
    }
    Ok(files)
}

fn verify(root: &Path, tag: &str, rustc: &str, wasm_pack: &str) -> Result<()> {
    let inventory: Inventory =
        serde_json::from_slice(&fs::read(root.join("docs/release-inventory.json"))?)?;
    let manifest: toml::Value = toml::from_str(&fs::read_to_string(root.join("Cargo.toml"))?)?;
    let version = manifest["package"]["version"]
        .as_str()
        .ok_or("missing package version")?;
    if inventory.version != version || tag != format!("v{version}") {
        return Err(
            "release tag/version differs from reviewed inventory; regenerate notices".into(),
        );
    }
    if inventory.source_commit.len() != 40
        || !inventory
            .source_commit
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
        || inventory.target != "wasm32-unknown-unknown"
        || inventory.features != ["ngg1", "interaction"]
        || inventory.rustc_version != rustc.trim()
        || inventory.wasm_pack_version != wasm_pack.trim()
    {
        return Err("release profile/toolchain differs from reviewed inventory".into());
    }
    if inventory.source_files != source_files(root)? {
        return Err(
            "release sources/lock/config differ from reviewed inventory; regenerate notices".into(),
        );
    }
    let expected = ["THIRD-PARTY-NOTICES.md", "docs/UPSTREAMS.md"];
    if inventory.notice_files.len() != expected.len() {
        return Err(
            "notice inventory must include exactly the full notices and source inventory".into(),
        );
    }
    for name in expected {
        if inventory.notice_files.get(name) != Some(&hash(&root.join(name))?) {
            return Err(format!("reviewed notice digest mismatch: {name}").into());
        }
    }
    Ok(())
}

fn package(root: &Path, tag: &str, rustc: &str, wasm_pack: &str) -> Result<()> {
    verify(root, tag, rustc, wasm_pack)?;
    let pkg = root.join("pkg");
    for path in [pkg.clone(), pkg.join("package.json"), pkg.join("docs")] {
        if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err("refusing npm package symlink".into());
        }
    }
    let path = pkg.join("package.json");
    let mut metadata: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
    if metadata["version"].as_str() != Some(tag.trim_start_matches('v')) {
        return Err("generated npm package version mismatch".into());
    }
    let files = metadata["files"]
        .as_array_mut()
        .ok_or("npm package needs an explicit files allowlist")?;
    for name in ["THIRD-PARTY-NOTICES.md", "docs/UPSTREAMS.md"] {
        if !files.iter().any(|v| v.as_str() == Some(name)) {
            files.push(name.into());
        }
    }
    fs::create_dir_all(pkg.join("docs"))?;
    for (src, dst) in [
        ("THIRD-PARTY-NOTICES.md", "THIRD-PARTY-NOTICES.md"),
        ("docs/UPSTREAMS.md", "docs/UPSTREAMS.md"),
    ] {
        let destination = pkg.join(dst);
        if fs::symlink_metadata(&destination).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err("refusing notice destination symlink".into());
        }
        fs::copy(root.join(src), destination)?;
    }
    fs::write(path, serde_json::to_vec_pretty(&metadata)?)?;
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 5 {
        return Err(
            "usage: vowl-release-inventory verify|package ROOT TAG RUSTC_VERSION WASM_PACK_VERSION"
                .into(),
        );
    }
    let root = Path::new(&args[1]);
    match args[0].as_str() {
        "verify" => verify(root, &args[2], &args[3], &args[4])?,
        "package" => package(root, &args[2], &args[3], &args[4])?,
        _ => return Err("unknown mode".into()),
    }
    println!("Reviewed release inventory verified");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fixture() -> TempDir {
        let temp = tempfile::tempdir().unwrap();
        let r = temp.path();
        fs::create_dir_all(r.join("src")).unwrap();
        fs::create_dir_all(r.join("docs")).unwrap();
        fs::create_dir_all(r.join("pkg")).unwrap();
        fs::write(r.join("Cargo.toml"), "[package]\nversion='0.2.0'\n").unwrap();
        fs::write(r.join("Cargo.lock"), "locked graph").unwrap();
        fs::write(r.join("src/lib.rs"), "// frozen source").unwrap();
        fs::write(r.join("THIRD-PARTY-NOTICES.md"), "complete notices").unwrap();
        fs::write(r.join("docs/UPSTREAMS.md"), "exact sources").unwrap();
        fs::write(
            r.join("pkg/package.json"),
            r#"{"version":"0.2.0","files":["vowl_wasm.js"]}"#,
        )
        .unwrap();
        let notice_files: BTreeMap<_, _> = ["THIRD-PARTY-NOTICES.md", "docs/UPSTREAMS.md"]
            .into_iter()
            .map(|p| (p, hash(&r.join(p)).unwrap()))
            .collect();
        let inventory = serde_json::json!({"version":"0.2.0","source_commit":"a".repeat(40),"target":"wasm32-unknown-unknown","features":["ngg1","interaction"],"rustc_version":"rustc reviewed","wasm_pack_version":"wasm-pack reviewed","source_files":source_files(r).unwrap(),"notice_files":notice_files});
        fs::write(
            r.join("docs/release-inventory.json"),
            serde_json::to_vec(&inventory).unwrap(),
        )
        .unwrap();
        temp
    }
    fn check(r: &Path) -> Result<()> {
        verify(r, "v0.2.0", "rustc reviewed", "wasm-pack reviewed")
    }

    #[test]
    fn accepts_reviewed_inputs_and_includes_notices_in_explicit_allowlist() {
        let t = fixture();
        let r = t.path();
        package(r, "v0.2.0", "rustc reviewed", "wasm-pack reviewed").unwrap();
        let p: serde_json::Value =
            serde_json::from_slice(&fs::read(r.join("pkg/package.json")).unwrap()).unwrap();
        assert_eq!(
            p["files"],
            serde_json::json!([
                "vowl_wasm.js",
                "THIRD-PARTY-NOTICES.md",
                "docs/UPSTREAMS.md"
            ])
        );
        assert_eq!(
            fs::read(r.join("pkg/THIRD-PARTY-NOTICES.md")).unwrap(),
            b"complete notices"
        );
        assert_eq!(
            fs::read(r.join("pkg/docs/UPSTREAMS.md")).unwrap(),
            b"exact sources"
        );
    }
    #[test]
    fn refuses_future_tag_and_toolchain() {
        let t = fixture();
        let r = t.path();
        assert!(verify(r, "v0.2.1", "rustc reviewed", "wasm-pack reviewed").is_err());
        assert!(verify(r, "v0.2.0", "rustc new", "wasm-pack reviewed").is_err());
        assert!(verify(r, "v0.2.0", "rustc reviewed", "wasm-pack new").is_err());
    }
    #[test]
    fn refuses_source_lock_and_new_file_drift() {
        for file in ["Cargo.lock", "src/lib.rs", "src/new.rs"] {
            let t = fixture();
            fs::write(t.path().join(file), "drift").unwrap();
            assert!(check(t.path()).is_err(), "{file}");
        }
    }
    #[test]
    fn refuses_notice_changes_before_package_writes() {
        let t = fixture();
        let r = t.path();
        fs::write(r.join("THIRD-PARTY-NOTICES.md"), "drift").unwrap();
        assert!(package(r, "v0.2.0", "rustc reviewed", "wasm-pack reviewed").is_err());
        assert!(!r.join("pkg/THIRD-PARTY-NOTICES.md").exists());
    }
    #[test]
    fn refuses_changed_release_feature_profile() {
        let t = fixture();
        let path = t.path().join("docs/release-inventory.json");
        let mut p: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        p["features"] = serde_json::json!(["parallel"]);
        fs::write(path, serde_json::to_vec(&p).unwrap()).unwrap();
        assert!(check(t.path()).is_err());
    }
    #[test]
    fn refuses_npm_version_mismatch() {
        let t = fixture();
        fs::write(
            t.path().join("pkg/package.json"),
            r#"{"version":"0.2.1","files":[]}"#,
        )
        .unwrap();
        assert!(package(t.path(), "v0.2.0", "rustc reviewed", "wasm-pack reviewed").is_err());
    }
}
