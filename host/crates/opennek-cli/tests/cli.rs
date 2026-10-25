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

mod options {
    use clap::Parser;
    use opennek_cli::cli::Args;

    fn parse(args: &[&str]) -> Args {
        Args::try_parse_from(std::iter::once("opennek").chain(args.iter().copied())).unwrap()
    }

    #[test]
    fn the_browser_opens_unless_told_not_to() {
        assert!(parse(&[]).opens_browser());
        assert!(!parse(&["--no-open"]).opens_browser());
    }

    #[test]
    fn open_is_still_accepted() {
        assert!(parse(&["--open"]).opens_browser());
    }

    #[test]
    fn defaults() {
        let a = parse(&[]);
        assert_eq!(
            (a.port, a.host.as_str(), a.sigfox.as_deref(), a.data.is_none()),
            (8085, "127.0.0.1", None, true)
        );
    }
}

#[test]
fn install_is_a_subcommand() {
    use clap::Parser;
    use opennek_cli::cli::{Args, Command};
    let a = Args::try_parse_from(["opennek", "install"]).unwrap();
    assert!(matches!(a.command, Some(Command::Install)));
    let a = Args::try_parse_from(["opennek", "uninstall"]).unwrap();
    assert!(matches!(a.command, Some(Command::Uninstall)));
    assert!(Args::try_parse_from(["opennek"]).unwrap().command.is_none());
}
