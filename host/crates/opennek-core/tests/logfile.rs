use std::fs;

use opennek_core::logfile::{LIMIT, LogFile, log_path};

const TIME: i64 = 1_760_000_000_123;

fn stamp(time: i64) -> String {
    chrono::DateTime::from_timestamp_millis(time)
        .unwrap()
        .with_timezone(&chrono::Local)
        .format("%Y-%m-%d %H:%M:%S%.3f")
        .to_string()
}

#[test]
fn the_log_is_in_the_logs_dir_and_rotates_at_5_mb() {
    let data = std::path::Path::new("/data");
    assert_eq!(log_path(data), data.join("logs").join("opennek.log"));
    assert_eq!(LIMIT, 5 * 1024 * 1024);
}

#[test]
fn lines_are_appended_with_date_and_time() {
    let data = tempfile::tempdir().unwrap();
    let path = log_path(data.path());
    let mut log = LogFile::new(path.clone(), LIMIT);
    assert_eq!(log.path(), path);
    assert!(log.append(TIME, "one").is_none());
    assert!(log.append(TIME + 1000, "two").is_none());
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        format!("{} one\n{} two\n", stamp(TIME), stamp(TIME + 1000))
    );

    // a log opened again appends to the file
    let mut log = LogFile::new(path.clone(), LIMIT);
    assert!(log.append(TIME, "three").is_none());
    let text = fs::read_to_string(&path).unwrap();
    assert_eq!(text.lines().count(), 3, "{text}");
    assert!(text.ends_with(" three\n"), "{text}");
}

#[test]
fn rotation_at_the_limit_keeps_one_old_file() {
    let data = tempfile::tempdir().unwrap();
    let path = log_path(data.path());
    let line_len = stamp(TIME).len() as u64 + " line 0\n".len() as u64;
    // room for three lines per file
    let mut log = LogFile::new(path.clone(), line_len * 3);
    for i in 0..3 {
        assert!(log.append(TIME, &format!("line {i}")).is_none());
    }
    assert!(!path.with_extension("log.1").exists());

    log.append(TIME, "line 3");
    let old = fs::read_to_string(path.with_extension("log.1")).unwrap();
    assert_eq!(old.lines().count(), 3, "{old}");
    assert!(old.ends_with(" line 2\n"), "{old}");
    assert_eq!(fs::read_to_string(&path).unwrap(), format!("{} line 3\n", stamp(TIME)));

    // the next rotation replaces the old file; there are never more than two
    for i in 4..7 {
        log.append(TIME, &format!("line {i}"));
    }
    let old = fs::read_to_string(path.with_extension("log.1")).unwrap();
    assert!(old.starts_with(&format!("{} line 3\n", stamp(TIME))), "{old}");
    assert_eq!(fs::read_to_string(&path).unwrap(), format!("{} line 6\n", stamp(TIME)));
    let mut files: Vec<String> = fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    files.sort();
    assert_eq!(files, ["opennek.log", "opennek.log.1"]);

    // a log opened again knows the size of the file
    let mut log = LogFile::new(path.clone(), line_len * 3);
    log.append(TIME, "line 7");
    log.append(TIME, "line 8");
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .starts_with(&format!("{} line 6\n", stamp(TIME)))
    );
    log.append(TIME, "line 9");
    assert_eq!(fs::read_to_string(&path).unwrap(), format!("{} line 9\n", stamp(TIME)));
}

#[test]
fn a_failure_is_reported_once_until_writing_works_again() {
    let data = tempfile::tempdir().unwrap();
    // the logs dir cannot be made: a file is in its place
    fs::write(data.path().join("logs"), "").unwrap();
    let mut log = LogFile::new(log_path(data.path()), LIMIT);
    assert!(log.append(TIME, "one").is_some());
    assert!(log.append(TIME, "two").is_none());

    fs::remove_file(data.path().join("logs")).unwrap();
    assert!(log.append(TIME, "three").is_none());
    assert_eq!(
        fs::read_to_string(log_path(data.path())).unwrap(),
        format!("{} three\n", stamp(TIME))
    );
    fs::remove_dir_all(data.path().join("logs")).unwrap();
    fs::write(data.path().join("logs"), "").unwrap();
    assert!(log.append(TIME, "four").is_some());
}
