use std::path::PathBuf;

use vmrp_abi::{MrChunk, MrpFile};

fn real_mrp_path() -> PathBuf {
    PathBuf::from(r"D:\opt\rust\vmrp\mrc\asm\asm.mrp")
}

#[test]
fn parses_real_start_mr_chunk_header_and_main_function() {
    let mrp = MrpFile::from_path(real_mrp_path()).unwrap();
    let assets = mrp.runtime_assets().unwrap();

    let chunk = MrChunk::from_bytes(assets.start_mr()).unwrap();
    assert_eq!(chunk.header().version(), 0x80);
    assert!(chunk.header().little_endian());

    let main = chunk.main();
    assert_eq!(main.source(), Some("@start.mr"));
    assert_eq!(main.line_defined(), 0);
    assert_eq!(main.nups(), 0);
    assert_eq!(main.num_params(), 0);
    assert_eq!(main.max_stack_size(), 9);
    assert!(main.code_count() > 0);
}

#[test]
fn rejects_invalid_mr_signature() {
    let err = MrChunk::from_bytes(b"NOT_MR_CHUNK").unwrap_err();
    assert!(format!("{err:?}").contains("InvalidSignature") || format!("{err:?}").contains("Truncated"));
}
