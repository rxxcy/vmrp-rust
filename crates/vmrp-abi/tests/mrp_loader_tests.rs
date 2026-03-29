use std::path::PathBuf;

use vmrp_abi::{ExtFile, MrpFile};

fn real_mrp_path() -> PathBuf {
    PathBuf::from(r"D:\opt\rust\vmrp\mrc\asm\asm.mrp")
}

fn real_ext_path() -> PathBuf {
    PathBuf::from(r"D:\opt\rust\vmrp\mrc\asm\cfunction.ext")
}

fn fallback_mrp_path() -> PathBuf {
    PathBuf::from(r"D:\opt\rust\vmrp\wasm\dist\fs\mythroad\ydqtwo.mrp")
}

fn fallback_ext_path() -> PathBuf {
    PathBuf::from(r"D:\opt\rust\vmrp\wasm\dist\fs\cfunction.ext")
}

#[test]
fn parses_real_mrp_header_and_file_directory() {
    let mrp = MrpFile::from_path(real_mrp_path()).unwrap();

    assert_eq!(mrp.magic(), b"MRPG");
    assert_eq!(mrp.internal_name(), "1.mrp");
    assert_eq!(mrp.app_name(), "asm");

    let names: Vec<&str> = mrp.entries().iter().map(|entry| entry.name()).collect();
    assert_eq!(names, vec!["start.mr", "cfunction.ext"]);
}

#[test]
fn extracts_cfunction_entry_payload_from_real_mrp() {
    let mrp = MrpFile::from_path(real_mrp_path()).unwrap();
    let payload = mrp.file_bytes("cfunction.ext").unwrap();

    assert_eq!(payload.len(), 0xA24);
    assert_eq!(&payload[..3], &[0x1F, 0x8B, 0x08]);
}

#[test]
fn inflates_cfunction_entry_payload_to_real_ext_bytes() {
    let mrp = MrpFile::from_path(real_mrp_path()).unwrap();
    let inflated = mrp.file_bytes_inflated("cfunction.ext").unwrap();
    let ext_bytes = std::fs::read(real_ext_path()).unwrap();

    assert_eq!(inflated, ext_bytes);
}

#[test]
fn builds_runtime_assets_from_real_mrp() {
    let mrp = MrpFile::from_path(real_mrp_path()).unwrap();
    let assets = mrp.runtime_assets().unwrap();

    assert_eq!(assets.cfunction_ext().header(), b"MRPGCMAP");
    assert_eq!(assets.start_mr().len(), 2490);
    assert_eq!(&assets.start_mr()[..4], &[0x1B, b'M', b'R', b'P']);
}

#[test]
fn builds_runtime_assets_with_external_helper_when_package_has_no_cfunction_ext() {
    let mrp = MrpFile::from_path(fallback_mrp_path()).unwrap();
    let ext = ExtFile::from_path(fallback_ext_path()).unwrap();
    let assets = mrp.runtime_assets_with_ext(ext).unwrap();

    assert_eq!(assets.cfunction_ext().header(), b"MRPGCMAP");
    assert!(assets.start_mr().len() > 1000);
    assert_eq!(&assets.start_mr()[..4], &[0x1B, b'M', b'R', b'P']);
}

#[test]
fn rejects_non_mrp_magic() {
    let err = MrpFile::from_bytes(b"NOT_MRP").unwrap_err();
    assert!(format!("{err:?}").contains("Truncated"));

    let mut bad = vec![0u8; 240];
    bad[..4].copy_from_slice(b"XRPG");
    let err = MrpFile::from_bytes(&bad).unwrap_err();
    assert!(format!("{err:?}").contains("InvalidHeader"));
}
