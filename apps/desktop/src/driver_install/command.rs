//! The exact privileged commands, safe quoting, source path validation and failure parsing.
//!
//! The administrator prompt runs one fixed AppleScript, `do shell script (item 1 of argv)
//! with administrator privileges`, and the shell command arrives as `argv` item 1, so
//! nothing is ever interpolated into AppleScript source. The shell command is assembled
//! here from fixed paths, with the one variable part (the bundled driver path) wrapped in
//! single quotes after validation. All of it is pure and unit tested.

use std::path::Path;

use super::error::DriverInstallError;
use super::model::{DRIVER_BUNDLE_ID, DRIVER_BUNDLE_NAME, HAL_DIR, installed_path, staging_path};

/// The AppleScript given to `osascript -e`; the shell command is passed as its first
/// argument. Fixed text: no user input and no path appears in it.
pub(crate) const ADMIN_WRAPPER_SCRIPT: &str =
    "on run argv\ndo shell script (item 1 of argv) with administrator privileges\nend run";

/// A failing step prints this marker and exits; `osascript` reports it in its error text.
const STEP_MARKER: &str = "prxvc-failed-step: ";

/// Wrap a string in single quotes so that `/bin/sh` reads it back as that exact string.
pub(crate) fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// Reject a path that must not be handed to the privileged command, and return it as text.
///
/// Requirements: absolute, valid UTF-8, no control character (newline and NUL included),
/// last component exactly `PrxVoiceMicrophone.driver`. Every other character, including
/// quotes, `$`, backticks, `;`, spaces and backslashes, is handled by [`shell_quote`].
pub(crate) fn validate_source(path: &Path) -> Result<&str, DriverInstallError> {
    let reject = |why: &str| Err(DriverInstallError::SourceRejected(why.to_owned()));
    let Some(text) = path.to_str() else {
        return reject("the driver path is not valid UTF-8");
    };
    if !path.is_absolute() {
        return reject("the driver path is not absolute");
    }
    if text.chars().any(char::is_control) {
        return reject("the driver path contains a control character");
    }
    if path.file_name().and_then(|n| n.to_str()) != Some(DRIVER_BUNDLE_NAME) {
        return reject("the driver bundle has an unexpected name");
    }
    if text.ends_with('/') {
        return reject("the driver path ends with a separator");
    }
    Ok(text)
}

/// A Team ID is ten upper-case letters or digits.
fn valid_team_id(team: &str) -> bool {
    team.len() == 10
        && team
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

/// Shell commands run as one line: a `fail` helper, then each step with `|| fail <step>`.
fn script(steps: &[String]) -> String {
    let mut out = format!(r#"fail() {{ echo "{STEP_MARKER}$1" >&2; exit 1; }}"#);
    for step in steps {
        out.push_str("; ");
        out.push_str(step);
    }
    out
}

/// The command that installs or replaces the driver, run as root.
///
/// Steps: stage a copy next to the destination; make it root-owned and not writable by
/// others; verify the staged copy (identifier, signature, and when `team_id` is given the
/// signing team) *after* the copy, so that a swap of the source between the unprivileged
/// check and the copy cannot get an unchecked bundle installed; replace the destination;
/// restart the audio daemon. Touches only `/Library/Audio/Plug-Ins/HAL/PrxVoiceMicrophone.driver`
/// and its staging directory.
pub(crate) fn install_command(
    source: &str,
    team_id: Option<&str>,
) -> Result<String, DriverInstallError> {
    let hal = shell_quote(HAL_DIR);
    let staging = shell_quote(&staging_path().to_string_lossy());
    let dest = shell_quote(&installed_path().to_string_lossy());
    let plist = shell_quote(&format!(
        "{}/Contents/Info.plist",
        staging_path().to_string_lossy()
    ));
    let requirement = match team_id {
        None => String::new(),
        Some(team) if valid_team_id(team) => format!(
            " -R {}",
            shell_quote(&format!(
                "=anchor apple generic and certificate leaf[subject.OU] = \"{team}\""
            ))
        ),
        Some(_) => {
            return Err(DriverInstallError::SourceRejected(
                "the signing team identifier is malformed".to_owned(),
            ));
        }
    };
    Ok(script(&[
        format!("/bin/mkdir -p {hal} || fail stage"),
        format!("/bin/rm -rf {staging} || fail stage"),
        format!(
            "/usr/bin/ditto {} {staging} || fail stage",
            shell_quote(source)
        ),
        format!("/usr/sbin/chown -R root:wheel {staging} || fail stage"),
        format!("/bin/chmod -R a+rX,go-w {staging} || fail stage"),
        format!(
            "[ \"$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' {plist})\" = {} ] || fail verify",
            shell_quote(DRIVER_BUNDLE_ID)
        ),
        format!("/usr/bin/codesign --verify --strict{requirement} {staging} || fail verify"),
        format!("/bin/rm -rf {dest} || fail swap"),
        format!("/bin/mv {staging} {dest} || fail swap"),
        restart_step(),
    ]))
}

/// The command that removes the driver and restarts the audio daemon, run as root.
pub(crate) fn uninstall_command() -> String {
    let dest = shell_quote(&installed_path().to_string_lossy());
    let staging = shell_quote(&staging_path().to_string_lossy());
    script(&[
        format!("/bin/rm -rf {dest} {staging} || fail remove"),
        restart_step(),
    ])
}

fn restart_step() -> String {
    "/bin/launchctl kickstart -k system/com.apple.audio.coreaudiod || /usr/bin/killall coreaudiod || fail restart"
        .to_owned()
}

/// Map the text `osascript` printed for a failed run to a typed error.
///
/// A dismissed prompt is `User canceled. (-128)`. Authorization errors carry codes in the
/// -60000 range. A step that failed inside the privileged script prints its marker.
pub(crate) fn classify_failure(stderr: &str, stdout: &str) -> DriverInstallError {
    let text = format!("{stderr}\n{stdout}");
    let lower = text.to_ascii_lowercase();
    if lower.contains("user canceled")
        || lower.contains("user cancelled")
        || lower.contains("(-128)")
    {
        return DriverInstallError::Cancelled;
    }
    if let Some(after) = text.split(STEP_MARKER).nth(1) {
        let step: String = after
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        return match step.as_str() {
            "restart" => DriverInstallError::RestartFailed(first_line(&text)),
            "verify" => DriverInstallError::SourceRejected(
                "the copied driver failed identifier or signature verification".to_owned(),
            ),
            _ => DriverInstallError::CopyFailed {
                step,
                detail: first_line(&text),
            },
        };
    }
    let auth_code = ["(-60005)", "(-60006)", "(-60007)", "(-60008)", "(-60031)"]
        .iter()
        .any(|code| text.contains(code));
    if auth_code || lower.contains("authoriz") || lower.contains("not allowed assistive") {
        return DriverInstallError::AuthorizationFailed(first_line(&text));
    }
    DriverInstallError::AuthorizationFailed(format!("unrecognised failure: {}", first_line(&text)))
}

/// First non-empty line, shortened, with the AppleScript prefix removed.
fn first_line(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("no details");
    let line = line
        .rsplit_once("execution error: ")
        .map_or(line, |(_, tail)| tail);
    line.chars().take(160).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;

    const GOOD: &str =
        "/Applications/PRX Voice.app/Contents/Resources/Drivers/PrxVoiceMicrophone.driver";

    #[test]
    fn quoting_round_trips_through_the_shell() {
        let hostile = [
            "plain",
            "with space",
            "it's",
            "'''",
            "a\"b",
            "$HOME",
            "`id`",
            "$(id)",
            "a;b&c|d",
            "back\\slash",
            "%s %d",
            "*?[x]",
            "ünï©ode 日本語",
            "-rf",
            "trailing\\",
            "'; rm -rf / #",
        ];
        for text in hostile {
            let out = Command::new("/bin/sh")
                .arg("-c")
                .arg(format!("printf %s {}", shell_quote(text)))
                .output()
                .expect("run sh");
            assert_eq!(
                String::from_utf8_lossy(&out.stdout),
                text,
                "quoting {text:?}"
            );
        }
    }

    #[test]
    fn source_validation() {
        assert_eq!(validate_source(Path::new(GOOD)), Ok(GOOD));
        let bad = [
            "relative/PrxVoiceMicrophone.driver",
            "/tmp/Other.driver",
            "/tmp/PrxVoiceMicrophone.driver/",
            "/tmp/new\nline/PrxVoiceMicrophone.driver",
            "/tmp/nul\0/PrxVoiceMicrophone.driver",
            "/tmp/tab\t/PrxVoiceMicrophone.driver",
            "/",
            "",
        ];
        for path in bad {
            assert!(
                matches!(
                    validate_source(Path::new(path)),
                    Err(DriverInstallError::SourceRejected(_))
                ),
                "{path:?}"
            );
        }
        // Quote-bearing paths are accepted and rely on quoting.
        let quoted = "/tmp/it's $HOME `x`;/PrxVoiceMicrophone.driver";
        assert_eq!(validate_source(Path::new(quoted)), Ok(quoted));
    }

    #[test]
    fn invalid_utf8_is_rejected() {
        use std::os::unix::ffi::OsStringExt;
        let mut bytes = b"/tmp/".to_vec();
        bytes.extend([0xff, 0xfe]);
        bytes.extend(b"/PrxVoiceMicrophone.driver");
        let path = PathBuf::from(std::ffi::OsString::from_vec(bytes));
        assert!(validate_source(&path).is_err());
    }

    #[test]
    fn install_command_is_exactly_this() {
        let command = install_command(
            "/Applications/PRX Voice.app/Contents/Resources/Drivers/PrxVoiceMicrophone.driver",
            None,
        )
        .expect("command");
        let expected = concat!(
            r#"fail() { echo "prxvc-failed-step: $1" >&2; exit 1; }"#,
            "; /bin/mkdir -p '/Library/Audio/Plug-Ins/HAL' || fail stage",
            "; /bin/rm -rf '/Library/Audio/Plug-Ins/HAL/.PrxVoiceMicrophone.driver.staging' || fail stage",
            "; /usr/bin/ditto '/Applications/PRX Voice.app/Contents/Resources/Drivers/PrxVoiceMicrophone.driver' '/Library/Audio/Plug-Ins/HAL/.PrxVoiceMicrophone.driver.staging' || fail stage",
            "; /usr/sbin/chown -R root:wheel '/Library/Audio/Plug-Ins/HAL/.PrxVoiceMicrophone.driver.staging' || fail stage",
            "; /bin/chmod -R a+rX,go-w '/Library/Audio/Plug-Ins/HAL/.PrxVoiceMicrophone.driver.staging' || fail stage",
            r#"; [ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' '/Library/Audio/Plug-Ins/HAL/.PrxVoiceMicrophone.driver.staging/Contents/Info.plist')" = 'org.openprx.prx-voice-client.microphone-driver' ] || fail verify"#,
            "; /usr/bin/codesign --verify --strict '/Library/Audio/Plug-Ins/HAL/.PrxVoiceMicrophone.driver.staging' || fail verify",
            "; /bin/rm -rf '/Library/Audio/Plug-Ins/HAL/PrxVoiceMicrophone.driver' || fail swap",
            "; /bin/mv '/Library/Audio/Plug-Ins/HAL/.PrxVoiceMicrophone.driver.staging' '/Library/Audio/Plug-Ins/HAL/PrxVoiceMicrophone.driver' || fail swap",
            "; /bin/launchctl kickstart -k system/com.apple.audio.coreaudiod || /usr/bin/killall coreaudiod || fail restart",
        );
        assert_eq!(command, expected);
        assert!(!command.contains('\n'));
    }

    #[test]
    fn hostile_source_stays_one_quoted_word() {
        let source = "/tmp/x'; rm -rf / #`id`$(id)/PrxVoiceMicrophone.driver";
        let command = install_command(source, None).expect("command");
        let quoted = shell_quote(source);
        assert!(command.contains(&format!("/usr/bin/ditto {quoted} ")));
        assert_eq!(
            command.matches(source).count(),
            0,
            "the raw path must not appear unquoted"
        );
        // The path appears exactly once, as one quoted word.
        assert_eq!(command.matches(&quoted).count(), 1);
    }

    #[test]
    fn commands_parse_as_shell_even_with_hostile_sources() {
        use std::io::Write;
        use std::process::Stdio;
        let sources = [
            GOOD,
            "/tmp/x'; rm -rf / #`id`$(id)/PrxVoiceMicrophone.driver",
            "/tmp/'\"'\"\\ \\/PrxVoiceMicrophone.driver",
        ];
        let mut commands: Vec<String> = sources
            .iter()
            .map(|s| install_command(s, Some("ABCDE12345")).expect("command"))
            .collect();
        commands.push(uninstall_command());
        for command in commands {
            // `sh -n` reads and parses without executing anything.
            let mut child = Command::new("/bin/sh")
                .arg("-n")
                .stdin(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("spawn sh");
            child
                .stdin
                .take()
                .expect("stdin")
                .write_all(command.as_bytes())
                .expect("write");
            let out = child.wait_with_output().expect("wait");
            assert!(
                out.status.success(),
                "{command}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }

    #[test]
    fn team_requirement_is_added_and_validated() {
        let with_team = install_command(GOOD, Some("ABCDE12345")).expect("command");
        assert!(with_team.contains(
            "/usr/bin/codesign --verify --strict -R '=anchor apple generic and certificate leaf[subject.OU] = \"ABCDE12345\"' "
        ));
        for bad in ["abcde12345", "ABCDE1234", "ABCDE12345'; id #", ""] {
            assert!(
                matches!(
                    install_command(GOOD, Some(bad)),
                    Err(DriverInstallError::SourceRejected(_))
                ),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn uninstall_command_is_exactly_this() {
        let expected = concat!(
            r#"fail() { echo "prxvc-failed-step: $1" >&2; exit 1; }"#,
            "; /bin/rm -rf '/Library/Audio/Plug-Ins/HAL/PrxVoiceMicrophone.driver' '/Library/Audio/Plug-Ins/HAL/.PrxVoiceMicrophone.driver.staging' || fail remove",
            "; /bin/launchctl kickstart -k system/com.apple.audio.coreaudiod || /usr/bin/killall coreaudiod || fail restart",
        );
        assert_eq!(uninstall_command(), expected);
    }

    #[test]
    fn the_fail_helper_prints_the_marker_that_the_parser_reads() {
        let out = Command::new("/bin/sh")
            .arg("-c")
            .arg(script(&["false || fail swap".to_owned()]))
            .output()
            .expect("run sh");
        assert!(!out.status.success());
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(matches!(
            classify_failure(&format!("execution error: {stderr} (1)"), ""),
            DriverInstallError::CopyFailed { ref step, .. } if step == "swap"
        ));
    }

    #[test]
    fn wrapper_script_has_no_interpolation_points() {
        assert_eq!(ADMIN_WRAPPER_SCRIPT.matches("item 1 of argv").count(), 1);
        assert!(!ADMIN_WRAPPER_SCRIPT.contains('&'));
        assert!(!ADMIN_WRAPPER_SCRIPT.contains('"'));
    }

    #[test]
    fn classifies_osascript_failures() {
        assert_eq!(
            classify_failure("0:27: execution error: User canceled. (-128)", ""),
            DriverInstallError::Cancelled
        );
        assert_eq!(
            classify_failure("execution error: user cancelled", ""),
            DriverInstallError::Cancelled
        );
        assert!(matches!(
            classify_failure(
                "execution error: The authorization was denied. (-60005)",
                ""
            ),
            DriverInstallError::AuthorizationFailed(_)
        ));
        assert!(matches!(
            classify_failure("execution error: prxvc-failed-step: restart (1)", ""),
            DriverInstallError::RestartFailed(_)
        ));
        assert!(matches!(
            classify_failure("execution error: prxvc-failed-step: verify (1)", ""),
            DriverInstallError::SourceRejected(_)
        ));
        assert!(matches!(
            classify_failure("execution error: prxvc-failed-step: stage (1)", ""),
            DriverInstallError::CopyFailed { ref step, .. } if step == "stage"
        ));
        assert!(matches!(
            classify_failure("", "something odd"),
            DriverInstallError::AuthorizationFailed(ref d) if d.contains("unrecognised")
        ));
    }

    #[test]
    fn failure_detail_is_short_and_single_line() {
        let long = format!(
            "execution error: prxvc-failed-step: swap {}\nsecond line",
            "x".repeat(500)
        );
        let DriverInstallError::CopyFailed { detail, .. } = classify_failure(&long, "") else {
            panic!("expected CopyFailed");
        };
        assert!(detail.chars().count() <= 160);
        assert!(!detail.contains('\n'));
    }
}
