use std::{
    fs,
    process::{Command, Output},
};
use tempfile::TempDir;

fn command(home: &TempDir) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_tnmy"));
    command
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("XDG_STATE_HOME", home.path().join("state"))
        .env_remove("TNMY_PALETTE")
        .env_remove("MYT_PALETTE")
        .env_remove("MYT_THEME_DIR")
        .env_remove("MYT_CONFIG");
    command
}
fn text(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

#[test]
fn redirected_output_is_plain_and_untruncated() {
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let name = format!("{}.txt", "длинное-имя".repeat(10));
    fs::write(dir.path().join(&name), "example").unwrap();
    let output = command(&home)
        .args(["ls"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(text(&output), format!("{name}\n"));
    assert!(!output.stdout.contains(&27));
}

#[test]
fn json_stays_structured_even_with_forced_color() {
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("file"), "abc").unwrap();
    let output = command(&home)
        .args(["--color", "always", "ls", "--json"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value[0]["size"], 3);
    assert!(!output.stdout.contains(&27));
}

#[test]
fn inaccessible_path_produces_failure_and_diagnostic() {
    let home = tempfile::tempdir().unwrap();
    let output = command(&home)
        .arg("ls")
        .arg(home.path().join("missing"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("missing"));
}

#[test]
fn browser_rejects_redirected_stdio() {
    let home = tempfile::tempdir().unwrap();
    let output = command(&home)
        .arg("files")
        .arg(home.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("interactive terminal"));
}

#[test]
fn explicit_missing_palette_is_an_error() {
    let home = tempfile::tempdir().unwrap();
    let output = command(&home)
        .arg("--palette")
        .arg(home.path().join("missing.json"))
        .arg("doctor")
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[cfg(unix)]
#[test]
fn zero_preserves_non_utf8_and_newline_bytes() {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir
        .path()
        .join(std::ffi::OsString::from_vec(b"odd\xff\nname".to_vec()));
    fs::write(&path, "").unwrap();
    let output = command(&home)
        .args(["ls", "--zero"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let mut expected = path.as_os_str().as_bytes().to_vec();
    expected.push(0);
    assert_eq!(output.stdout, expected);
    let json = command(&home)
        .args(["ls", "--json"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(!json.status.success());
    assert!(json.stdout.is_empty());
}

#[cfg(unix)]
#[test]
fn update_passes_literal_arguments_and_propagates_child_failure() {
    use std::os::unix::fs::PermissionsExt;
    let home = tempfile::tempdir().unwrap();
    let script = home.path().join("nh");
    let shell = std::env::var("TNMY_TEST_SH").unwrap_or_else(|_| "/bin/sh".into());
    fs::write(
        &script,
        format!("#!{shell}\nprintf '%s\\n' \"$@\"\nexit 37\n"),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let output = command(&home)
        .env("PATH", home.path())
        .args([
            "update",
            "--flake",
            "a b; echo BAD",
            "--action",
            "build",
            "--",
            "--offline",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(37));
    assert_eq!(text(&output), "os\nbuild\na b; echo BAD\n--offline\n");
    assert!(String::from_utf8_lossy(&output.stderr).contains("exit 37"));
}

#[test]
fn dry_run_does_not_execute_nh() {
    let home = tempfile::tempdir().unwrap();
    let output = command(&home)
        .env("PATH", home.path())
        .args(["update", "--dry-run", "--refresh", "--flake", "/etc/nixos"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let args: Vec<String> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(args, vec!["nh", "os", "switch", "--update", "/etc/nixos"]);
}

#[test]
fn discovers_configured_immutable_palette() {
    let home = tempfile::tempdir().unwrap();
    let directory = home.path().join("theme");
    fs::create_dir(&directory).unwrap();
    let keys = [
        "primary",
        "on_primary",
        "primary_container",
        "on_primary_container",
        "secondary",
        "secondary_container",
        "on_secondary_container",
        "tertiary",
        "surface",
        "surface_container",
        "surface_container_high",
        "on_surface",
        "on_surface_variant",
        "outline",
        "error",
    ];
    let colors: serde_json::Map<String, serde_json::Value> = keys
        .into_iter()
        .map(|k| (k.into(), "#abcdef".into()))
        .collect();
    fs::write(
        directory.join("palette.json"),
        serde_json::to_vec(&serde_json::json!({"mode":"light","colors":colors})).unwrap(),
    )
    .unwrap();
    let config_dir = home.path().join("config/material-you-terminal");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join("config.json"),
        serde_json::to_vec(&serde_json::json!({"themeDir":directory})).unwrap(),
    )
    .unwrap();
    let output = command(&home).args(["doctor", "--json"]).output().unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["mode"], "light");
    assert_eq!(
        report["palette"],
        directory.join("palette.json").to_str().unwrap()
    );
}
