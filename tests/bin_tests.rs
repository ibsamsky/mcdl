use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn test_help() {
    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();
    cmd.arg("--help");
    cmd.assert().success().stdout(predicate::str::contains(
        "A tool for managing Minecraft server versions",
    ));
}

#[test]
fn test_version() {
    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();
    cmd.arg("--version");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn test_list() {
    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();
    cmd.arg("list");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("1.19.4").and(predicate::str::contains("23w13a").not()));
}

#[test]
fn test_list_filter() {
    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();
    cmd.arg("list").arg("--snapshot");
    cmd.assert().success().stdout(
        predicate::str::contains("1.19.4")
            .not()
            .and(predicate::str::contains("23w13a")),
    );
}

#[test]
fn test_info() {
    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();
    cmd.arg("info").arg("--version").arg("1.19.4");
    cmd.assert().success().stdout(
        predicate::str::contains("Version 1.19.4 (release)")
            .and(predicate::str::contains("Released: 14 March 2023")),
    );
}

#[test]
fn test_locate_config() {
    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();
    cmd.arg("locate").arg("config");
    match std::env::consts::OS {
        "windows" => {
            cmd.assert()
                .success()
                .stdout(predicate::str::contains("AppData\\Local"));
        }
        "linux" => {
            cmd.assert()
                .success()
                .stdout(predicate::str::contains(".config"));
        }
        "macos" => {
            cmd.assert()
                .success()
                .stdout(predicate::str::contains("Library/Application Support"));
        }
        _ => {
            panic!("Unsupported OS");
        }
    }
}
