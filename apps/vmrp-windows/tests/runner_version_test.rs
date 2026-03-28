use std::process::Command;

fn run_sample(path: &str) -> String {
    let exe = env!("CARGO_BIN_EXE_vmrp-windows");
    let output = Command::new(exe)
        .arg(path)
        .output()
        .expect("run vmrp-windows");
    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn asm_sample_bootstrap_succeeds() {
    let stdout = run_sample(r"D:\opt\rust\vmrp\mrc\asm\asm.mrp");
    assert!(
        stdout.contains("mrp_bootstrap_run_ok=true"),
        "stdout was:\n{stdout}"
    );
}

#[test]
fn asm_thumb_sample_bootstrap_succeeds() {
    let stdout = run_sample(r"D:\opt\rust\vmrp\mrc\asm\asm_thumb.mrp");
    assert!(
        stdout.contains("mrp_bootstrap_run_ok=true"),
        "stdout was:\n{stdout}"
    );
}
