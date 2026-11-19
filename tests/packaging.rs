#![cfg(all(feature = "desktop", target_os = "macos"))]
use std::process::Command;

#[test]
fn packaging_produces_a_signed_macos_app_with_a_launchable_executable() {
    let temp = tempfile::TempDir::new().unwrap();
    let bundle = temp.path().join("Katastro.app");
    // Repack over an older installation with a retired runtime notice.
    let obsolete = bundle.join("Contents/Resources/Notices/retired-backend.txt");
    std::fs::create_dir_all(obsolete.parent().unwrap()).unwrap();
    std::fs::write(&obsolete, "previous release").unwrap();
    let status = Command::new("bash")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/package-macos.sh"
        ))
        .arg(&bundle)
        .env("KATASTRO_BINARY", env!("CARGO_BIN_EXE_katastro"))
        .status()
        .unwrap();
    assert!(status.success());
    for resource in [
        "Helpers/katago",
        "Resources/Models/review.bin.gz",
        "Resources/Models/b18c384nbt-humanv0.bin.gz",
        "Resources/Notices/models.LICENSE",
    ] {
        assert!(
            bundle.join("Contents").join(resource).is_file(),
            "{resource}"
        );
    }
    assert!(
        !obsolete.exists(),
        "obsolete runtime notice survived repackaging"
    );
    let plist = bundle.join("Contents/Info.plist");
    assert!(
        plist.is_file(),
        "a launchable app needs its bundle metadata"
    );
    let name = Command::new("plutil")
        .args(["-extract", "CFBundleExecutable", "raw", "-o", "-"])
        .arg(&plist)
        .output()
        .unwrap();
    assert!(name.status.success());
    let binary = bundle
        .join("Contents/MacOS")
        .join(String::from_utf8(name.stdout).unwrap().trim());
    let helper = bundle.join("Contents/Helpers/katago");
    for executable in [&binary, &helper] {
        let arches = Command::new("/usr/bin/lipo")
            .arg("-archs")
            .arg(executable)
            .output()
            .unwrap();
        assert!(arches.status.success());
        assert_eq!(
            String::from_utf8(arches.stdout).unwrap().trim(),
            "arm64",
            "{}",
            executable.display()
        );
    }
    let version = Command::new(helper).arg("version").output().unwrap();
    assert!(version.status.success());
    assert!(
        String::from_utf8(version.stdout)
            .unwrap()
            .contains("Using Metal backend")
    );
    assert!(
        Command::new("test")
            .arg("-x")
            .arg(&binary)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(&bundle)
            .status()
            .unwrap()
            .success()
    );
}

#[test]
#[ignore = "requires an Apple Silicon Mac with a working Metal device"]
fn relocated_runtime_runs_review_and_humansl_from_its_own_bundle() {
    use katastro::{
        engine::{Engine, EngineConfig, EngineEvent},
        training::client::Options,
    };
    use std::time::Duration;
    if let Some(root) = std::env::var_os("KATASTRO_RELOCATED_TEST") {
        let root = std::path::PathBuf::from(root);
        use katastro::worker::{Client, Command as ReviewCommand};
        let defaults = EngineConfig::discover(&root);
        let client = Client::spawn(root.join("reviews"), root.join("cache"), defaults.clone());
        let config = client.recv(Duration::from_secs(5)).unwrap().config;
        let game = root.join("example.sgf");
        let check_review = || {
            let end = std::time::Instant::now() + Duration::from_secs(120);
            loop {
                let snapshot = client
                    .recv(end.saturating_duration_since(std::time::Instant::now()))
                    .unwrap();
                assert!(snapshot.error.is_none(), "{:?}", snapshot.error);
                if !snapshot.running
                    && snapshot.values.len() == 2
                    && snapshot.values.values().all(|a| a.visits >= 64)
                {
                    break;
                }
            }
        };
        if std::env::var_os("SAVE_RUNTIME_SETTINGS").is_some() {
            let mut settings = defaults;
            settings.analysis.max_visits = Some(64);
            client.send(ReviewCommand::Configure(settings)).unwrap();
            assert_eq!(
                client
                    .recv(Duration::from_secs(5))
                    .unwrap()
                    .config
                    .analysis
                    .max_visits,
                Some(64)
            );
            std::fs::write(&game, "(;GM[1]FF[4]SZ[9]KM[7.5]RU[Chinese];B[ee])").unwrap();
            client.send(ReviewCommand::OpenAndAnalyze(game)).unwrap();
            check_review();
            return;
        }
        assert_eq!(config.analysis.max_visits, Some(64));
        let contents = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_owned();
        assert!(
            config.executable.starts_with(contents.join("Helpers")),
            "engine escaped the relocated bundle"
        );
        assert!(
            config.model.starts_with(contents.join("Resources")),
            "review model escaped the relocated bundle"
        );
        client.send(ReviewCommand::Open(game)).unwrap();
        check_review();
        drop(client);
        let training = Options::new(root.join("reviews"), root.join("cache"), config.clone());
        assert!(
            training.human_model.starts_with(contents.join("Resources")),
            "training needs a bundled model"
        );
        let mut engine = Engine::spawn_with_human(&config, &training.human_model).unwrap();
        engine.send(&serde_json::json!({"id":"relocated","boardXSize":9,"boardYSize":9,"moves":[],"rules":"chinese","komi":7.5,"maxVisits":64,"includePolicy":true,"overrideSettings":{"humanSLProfile":"rank_1d"}})).unwrap();
        loop {
            match engine.recv_timeout(Duration::from_secs(120)).unwrap() {
                EngineEvent::Response(reply) => {
                    assert_eq!(reply["rootInfo"]["visits"], 64, "{reply}");
                    assert_eq!(reply["humanPolicy"].as_array().unwrap().len(), 82);
                    break;
                }
                EngineEvent::Diagnostic(_) => {}
                event => panic!("{event:?}"),
            }
        }
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let bundle = temp.path().join("Original.app");
    assert!(
        Command::new("bash")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/scripts/package-macos.sh"
            ))
            .arg(&bundle)
            .env("KATASTRO_BINARY", env!("CARGO_BIN_EXE_katastro"))
            .status()
            .unwrap()
            .success()
    );
    let probe = "Contents/MacOS/katastro";
    std::fs::copy(std::env::current_exe().unwrap(), bundle.join(probe)).unwrap();
    assert!(
        Command::new("codesign")
            .args(["--force", "--sign", "-"])
            .arg(&bundle)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(&bundle)
            .status()
            .unwrap()
            .success()
    );
    let run = |app: &std::path::Path, save| {
        let mut cmd = Command::new("/usr/bin/sandbox-exec");
        cmd.args(["-p","(version 1)(allow default)(deny network*)(deny file-read* (subpath \"/opt/homebrew\") (subpath \"/usr/local\"))"])
            .arg(app.join(probe)).args(["--exact","relocated_runtime_runs_review_and_humansl_from_its_own_bundle","--include-ignored","--nocapture"])
            .env("HOME", temp.path()).env("PATH", "/usr/bin:/bin").env("KATASTRO_RELOCATED_TEST",temp.path());
        if save {
            cmd.env("SAVE_RUNTIME_SETTINGS", "1");
        }
        assert!(cmd.status().unwrap().success());
    };
    run(&bundle, true);
    let moved = temp.path().join("Relocated Katastro.app");
    std::fs::rename(bundle, &moved).unwrap();
    run(&moved, false);
}

#[test]
fn clean_build_staging_repairs_from_verified_cache_and_rejects_corrupt_downloads() {
    use std::fs;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    for (path, data) in [
        ("Helpers/katago", "helper"),
        ("Resources/Models/review.bin.gz", "review"),
        ("Resources/Models/b18c384nbt-humanv0.bin.gz", "human"),
        ("Resources/Notices/models.LICENSE", "notice"),
    ] {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, data).unwrap();
    }
    let source = root.join("runtime.tar.gz");
    let mut tar = Command::new("/usr/bin/tar");
    assert!(
        tar.current_dir(root)
            .env("COPYFILE_DISABLE", "1")
            .args(["-czf", "runtime.tar.gz", "Helpers", "Resources"])
            .status()
            .unwrap()
            .success()
    );
    let sha = katastro::digest(&fs::read(&source).unwrap());
    let cache = root.join("cache");
    let profile = root.join("profile");
    fs::create_dir(&profile).unwrap();
    let lock = serde_json::json!({"url":format!("file://{}", source.display()),"sha256":sha});
    let script = root.join("stage.py");
    fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/runtime/stage.py"),
        &script,
    )
    .unwrap();
    fs::write(root.join("macos.json"), lock.to_string()).unwrap();
    let stage = |offline| {
        Command::new("/usr/bin/python3")
            .arg(&script)
            .arg(&profile)
            .env("KATASTRO_RUNTIME_CACHE", &cache)
            .env("KATASTRO_OFFLINE", if offline { "1" } else { "0" })
            .env_remove("CARGO_NET_OFFLINE")
            .output()
            .unwrap()
    };
    let result = stage(false);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let helper = profile.join(format!("katastro-runtime-{sha}/Helpers/katago"));
    fs::write(&source, "unexpected release bytes").unwrap();
    fs::remove_file(&helper).unwrap();
    assert!(stage(true).status.success());
    assert_eq!(fs::read(&helper).unwrap(), b"helper");
    fs::write(cache.join(format!("{sha}.tar.gz")), "damaged cache").unwrap();
    assert!(!stage(true).status.success());
    let bad_download = stage(false);
    assert!(!bad_download.status.success());
    assert!(String::from_utf8_lossy(&bad_download.stderr).contains("checksum mismatch"));
    assert_eq!(fs::read(&helper).unwrap(), b"helper");
}
