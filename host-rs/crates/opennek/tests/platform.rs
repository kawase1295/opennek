use std::path::PathBuf;

use opennek::platform::{Os, browser_command, default_data_dir};

fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let vars: Vec<(String, String)> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    move |k| vars.iter().find(|(name, _)| name == k).map(|(_, v)| v.clone())
}

#[test]
fn the_data_directory_follows_each_os() {
    let dir = |os, vars: &[(&str, &str)], home| default_data_dir(os, env(vars), home);
    assert_eq!(
        dir(Os::Linux, &[], "/home/u"),
        PathBuf::from("/home/u/.local/share/opennek")
    );
    assert_eq!(
        dir(Os::Linux, &[("XDG_DATA_HOME", "/data")], "/home/u"),
        PathBuf::from("/data/opennek")
    );
    assert_eq!(
        dir(
            Os::Windows,
            &[("APPDATA", r"C:\Users\u\AppData\Roaming")],
            r"C:\Users\u"
        ),
        PathBuf::from(r"C:\Users\u\AppData\Roaming\opennek")
    );
    assert_eq!(
        dir(Os::Windows, &[], r"C:\Users\u"),
        PathBuf::from(r"C:\Users\u\AppData\Roaming\opennek")
    );
    assert_eq!(
        dir(Os::MacOs, &[], "/Users/u"),
        PathBuf::from("/Users/u/Library/Application Support/opennek")
    );
}

#[test]
fn the_browser_is_opened_with_the_os_command() {
    let url = "http://127.0.0.1:8085/";
    assert_eq!(browser_command(Os::Linux, url), ("xdg-open", vec![url.to_string()]));
    assert_eq!(browser_command(Os::MacOs, url), ("open", vec![url.to_string()]));
    // start is built into cmd; the empty argument is the window title
    assert_eq!(
        browser_command(Os::Windows, url),
        ("cmd", vec!["/c".into(), "start".into(), String::new(), url.to_string()])
    );
}
