use std::process::Command;

fn opennek(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_opennek"))
        .args(args)
        .output()
        .expect("run opennek")
}

#[test]
fn prints_the_package_version() {
    let out = opennek(&["--version"]);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        format!("opennek {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn rejects_an_unknown_argument() {
    let out = opennek(&["--nope"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--nope"));
}
