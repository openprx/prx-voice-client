use super::*;
use crate::test_util::TempDir;
use crate::AppConfig;

#[test]
fn app_id_and_bundle_identifier() {
    assert_eq!(APP_ID, ("org", "openprx", "prx-voice-client"));
    assert_eq!(bundle_identifier(), "org.openprx.prx-voice-client");
}

#[test]
fn resolved_dirs_use_the_new_identifier() {
    let paths = AppPaths::resolve_with(&AppConfig::default(), None).unwrap();
    let cfg = paths.config_dir.to_string_lossy().into_owned();
    assert!(cfg.contains("prx-voice-client"), "{cfg}");
    assert!(!cfg.contains("com.prx"), "{cfg}");
    #[cfg(target_os = "macos")]
    {
        assert!(cfg.ends_with("org.openprx.prx-voice-client"), "{cfg}");
        assert_eq!(paths.data_dir, paths.config_dir);
        assert_eq!(paths.models_dir, paths.data_dir.join("models"));
    }
    assert_eq!(paths.voices_dir, paths.data_dir.join("voices"));
    assert_eq!(paths.logs_dir, paths.data_dir.join("logs"));
}

#[test]
fn override_and_models_dir_setting() {
    let root = Path::new("/nonexistent/prxvc-root");
    let paths = AppPaths::resolve_with(&AppConfig::default(), Some(root)).unwrap();
    assert_eq!(paths.data_dir, root);
    assert_eq!(paths.models_dir, root.join("models"));
    let mut config = AppConfig::default();
    config.models.dir = "/nonexistent/elsewhere".into();
    let paths = AppPaths::resolve_with(&config, Some(root)).unwrap();
    assert_eq!(paths.models_dir, Path::new("/nonexistent/elsewhere"));
    assert_eq!(paths.data_dir, root);
}

#[test]
fn ensure_dirs_creates_private_directories() {
    let f = TempDir::new("ensure");
    let paths = AppPaths::from_roots(f.path().join("c"), f.path().join("d"), f.path().join("d/models"), &AppConfig::default());
    paths.ensure_dirs().unwrap();
    for d in [&paths.config_dir, &paths.data_dir, &paths.models_dir, &paths.voices_dir, &paths.logs_dir] {
        assert!(d.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(d).unwrap().permissions().mode() & 0o777, 0o700);
        }
    }
}
