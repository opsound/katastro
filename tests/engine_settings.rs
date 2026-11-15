use katastro::{
    Analysis, Document, Review,
    engine::{AnalysisSettings, Engine, EngineConfig, EngineEvent},
    scheduler::Scheduler,
    training::{Phase, Settings},
    worker::{Client, Command, Snapshot},
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    path::Path,
    time::{Duration, Instant},
};
#[path = "support/training.rs"]
mod support;

fn config(root: &Path) -> EngineConfig {
    let model = root.join("model");
    std::fs::write(&model, b"settings-fixture").unwrap();
    EngineConfig {
        executable: env!("CARGO_BIN_EXE_katastro-test-engine").into(),
        model,
        analysis: Default::default(),
    }
}
fn until(c: &Client, predicate: impl Fn(&Snapshot) -> bool) -> Snapshot {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        let s = c
            .recv(end.saturating_duration_since(Instant::now()))
            .expect("worker must reach the configured analysis depth");
        assert!(s.error.is_none(), "{:?}", s.error);
        if predicate(&s) {
            return s;
        }
    }
}
fn finish(s: &mut Scheduler, q: &katastro::scheduler::Request) {
    for &turn in q.targets.keys() {
        s.accept(&json!({"id":q.id,"turnNumber":turn,"isDuringSearch":false,
            "rootInfo":{"visits":q.visits,"winrate":0.5,"scoreLead":1.0},"ownership":vec![0.0;81]}))
            .unwrap();
    }
}

// A user can bound review work, revisit analyzed positions without more work,
// and analyze a new variation to the same non-tier depth after restarting.
#[test]
fn review_depth_is_bounded_persisted_and_reused_across_reopen_and_variations() {
    let temp = tempfile::tempdir().unwrap();
    let source = support::fixture(temp.path(), false);
    let mut cfg = config(temp.path());
    cfg.analysis = AnalysisSettings {
        max_visits: Some(100),
        parallel_positions: 2,
        threads_per_position: 3,
    };
    let c = Client::spawn(
        temp.path().join("reviews"),
        temp.path().join("cache"),
        cfg.clone(),
    );
    c.send(Command::Configure(cfg.clone())).unwrap();
    c.send(Command::OpenAndAnalyze(source.clone())).unwrap();
    let s = until(&c, |s| {
        assert!(
            s.analysis_target.is_none_or(|v| v <= 100),
            "review ignored the visit limit"
        );
        s.coverage == (15, 15) && !s.running && s.values.values().all(|v| v.visits >= 100)
    });
    assert!(s.values.values().all(|v| v.visits == 100));
    assert!(
        s.status.contains("100"),
        "completion should identify the chosen limit"
    );
    drop(c);
    let c = Client::spawn(
        temp.path().join("reviews"),
        temp.path().join("cache"),
        EngineConfig::default(),
    );
    let restored = until(&c, |s| s.config.analysis.max_visits == Some(100));
    assert_eq!(restored.config.analysis, cfg.analysis);
    c.send(Command::OpenAndAnalyze(source.clone())).unwrap();
    until(&c, |s| s.coverage == (15, 15) && !s.running);
    c.send(Command::Select(2)).unwrap();
    until(&c, |s| {
        s.document.as_ref().is_some_and(|d| d.selected == 2) && !s.running
    });
    c.send(Command::Play(None)).unwrap();
    let branch = until(&c, |s| {
        s.values.get(&15).is_some_and(|a| a.visits == 100) && !s.running
    });
    assert_eq!(branch.document.as_ref().unwrap().mainline.len(), 15);
    let mut review = Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    review.import(&source).unwrap();
    let cached = review
        .cached_analysis(&Engine::profile(&cfg).unwrap())
        .unwrap();
    assert_eq!(cached.len(), 16);
    let mut scheduler = Scheduler::new(
        10,
        review.document().unwrap().clone(),
        Engine::profile(&cfg).unwrap(),
        cached,
    )
    .with_settings(&cfg.analysis)
    .unwrap();
    assert!(
        scheduler.next_request().unwrap().is_none(),
        "a compatible reopen needs no duplicate query"
    );
    assert!(scheduler.is_complete());
    cfg.analysis.max_visits = Some(200);
    c.send(Command::Configure(cfg)).unwrap();
    until(&c, |s| s.config.analysis.max_visits == Some(200));
    c.send(Command::Start).unwrap();
    until(&c, |s| {
        assert!(
            s.analysis_target.is_none_or(|v| (100..=200).contains(&v)),
            "raising the limit reuses earlier effort"
        );
        !s.running && s.values.len() == 16 && s.values.values().all(|v| v.visits >= 200)
    });
}

#[test]
fn parallel_positions_bound_refinement_and_selection_without_delaying_chart_coverage() {
    for parallel in [1, 2, 12] {
        let temp = tempfile::tempdir().unwrap();
        let doc =
            Document::parse(&std::fs::read(support::fixture(temp.path(), false)).unwrap()).unwrap();
        let settings = AnalysisSettings {
            parallel_positions: parallel,
            max_visits: Some(100),
            ..Default::default()
        };
        let mut s = Scheduler::new(1, doc.clone(), support::profile(), BTreeMap::new())
            .with_settings(&settings)
            .unwrap();
        let chart = s.next_request().unwrap().unwrap();
        assert_eq!(chart.visits, 1);
        assert_eq!(chart.targets.len(), 15);
        assert!(s.next_request().unwrap().is_none());
        finish(&mut s, &chart);
        let batch = s.next_request().unwrap().unwrap();
        assert_eq!(
            batch.targets.len(),
            parallel,
            "refinement must fill the chosen capacity"
        );
        assert!(s.next_request().unwrap().is_none());
        assert_eq!(s.pending(), parallel);
        let mut selected = doc;
        selected.selected = 14;
        s.select(selected);
        let interactive = s.next_request().unwrap().unwrap();
        assert_eq!(interactive.query["priority"], 100);
        assert_eq!(interactive.targets.values().next().unwrap()[0].node, 14);
        assert!(
            s.pending() <= parallel,
            "reserve capacity for the selected position"
        );
    }
}

#[test]
fn capped_selection_backfills_ownership_without_replacing_deeper_analysis() {
    let doc = Document::parse(b"(;SZ[9])").unwrap();
    let old = Analysis {
        visits: 4096,
        score_lead: 9.0,
        winrate: 0.7,
        suggestions: vec![],
        ownership: vec![],
        ownership_visits: 0,
    };
    let settings = AnalysisSettings {
        max_visits: Some(64),
        ..Default::default()
    };
    let mut s = Scheduler::new(1, doc.clone(), support::profile(), [(0, old)].into())
        .with_settings(&settings)
        .unwrap();
    let map = s.next_request().unwrap().unwrap();
    assert_eq!(map.visits, 64);
    finish(&mut s, &map);
    s.select(doc);
    assert!(
        s.next_request().unwrap().is_none(),
        "selection cannot exceed the configured cap"
    );
    assert!(s.is_complete());
    assert_eq!(s.values[&0].visits, 4096);
    assert_eq!(s.values[&0].score_lead, 9.0);
    assert_eq!(s.values[&0].ownership.len(), 81);
}

#[test]
fn runtime_controls_reach_the_engine_and_reuse_existing_cache_identity() {
    let temp = tempfile::tempdir().unwrap();
    let mut cfg = config(temp.path());
    let profile = Engine::profile(&cfg).unwrap();
    let source = support::fixture(temp.path(), false);
    let mut review = Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    review.import(&source).unwrap();
    review
        .store_analysis(
            &review.analysis_key(0, &profile).unwrap(),
            &Analysis {
                visits: 512,
                winrate: 0.5,
                score_lead: 1.0,
                suggestions: vec![],
                ownership: vec![0.0; 81],
                ownership_visits: 512,
            },
        )
        .unwrap();
    cfg.analysis = AnalysisSettings {
        max_visits: Some(100),
        parallel_positions: 2,
        threads_per_position: 3,
    };
    let mut engine = Engine::spawn(&cfg).unwrap();
    engine
        .send(&json!({"id":"configuration","action":"query_config"}))
        .unwrap();
    let actual = loop {
        if let EngineEvent::Response(r) = engine.recv_timeout(Duration::from_secs(5)).unwrap() {
            break r["config"].as_str().unwrap().to_owned();
        }
    };
    for setting in [
        "numAnalysisThreads = 2",
        "numSearchThreadsPerAnalysisThread = 3",
        "nnMaxBatchSize = 6",
    ] {
        assert!(
            actual.lines().any(|line| line == setting),
            "engine did not receive {setting}"
        );
    }
    let cached = review
        .cached_analysis(&Engine::profile(&cfg).unwrap())
        .unwrap();
    assert_eq!(
        cached.get(&0).map(|a| a.visits),
        Some(512),
        "effort controls must not discard compatible analysis"
    );
}

#[test]
fn invalid_settings_do_not_replace_saved_configuration_or_stop_running_review() {
    let temp = tempfile::tempdir().unwrap();
    let cfg = config(temp.path());
    std::fs::write(&cfg.model, b"hold-final").unwrap();
    let c = Client::spawn(
        temp.path().join("reviews"),
        temp.path().join("cache"),
        cfg.clone(),
    );
    c.send(Command::Configure(cfg.clone())).unwrap();
    until(&c, |s| s.status.starts_with("Engine settings saved"));
    c.send(Command::OpenAndAnalyze(support::fixture(
        temp.path(),
        false,
    )))
    .unwrap();
    until(&c, |s| {
        s.running && s.values.get(&0).is_some_and(|a| a.visits >= 64)
    });
    let invalid = [
        AnalysisSettings {
            max_visits: Some(63),
            ..Default::default()
        },
        AnalysisSettings {
            max_visits: Some(1_048_577),
            ..Default::default()
        },
        AnalysisSettings {
            parallel_positions: 0,
            ..Default::default()
        },
        AnalysisSettings {
            parallel_positions: 33,
            ..Default::default()
        },
        AnalysisSettings {
            threads_per_position: 0,
            ..Default::default()
        },
        AnalysisSettings {
            threads_per_position: 17,
            ..Default::default()
        },
    ];
    for analysis in invalid {
        let mut bad = cfg.clone();
        bad.analysis = analysis;
        c.send(Command::Configure(bad)).unwrap();
        let state = c.recv(Duration::from_secs(5)).unwrap();
        assert!(
            state.error.is_some(),
            "invalid engine settings were accepted"
        );
        assert_eq!(state.config.analysis, cfg.analysis);
        assert!(
            state.running,
            "invalid settings must leave the active review running"
        );
    }
    drop(c);
    let restored = Client::spawn(
        temp.path().join("reviews"),
        temp.path().join("cache"),
        EngineConfig::default(),
    );
    assert_eq!(
        restored
            .recv(Duration::from_secs(5))
            .unwrap()
            .config
            .analysis,
        cfg.analysis
    );
}

#[test]
fn training_uses_chosen_capacity_but_keeps_its_fixed_grading_budget() {
    for parallel in [1, 2] {
        let temp = tempfile::tempdir().unwrap();
        let mut t = support::trainer(temp.path())
            .with_analysis_settings(&AnalysisSettings {
                max_visits: Some(64),
                parallel_positions: parallel,
                threads_per_position: 1,
            })
            .unwrap();
        t.begin(
            vec![support::fixture(temp.path(), false)],
            "me".into(),
            Settings {
                moves: 1,
                ..Default::default()
            },
        )
        .unwrap();
        let initial: Vec<_> = (0..parallel)
            .map(|_| t.next_request().unwrap().unwrap())
            .collect();
        assert!(
            t.next_request().unwrap().is_none(),
            "discovery exceeded configured capacity"
        );
        for q in initial {
            t.accept(&support::reply(&q, false, true), 0.5).unwrap();
        }
        support::settle(&mut t, false, true);
        assert_eq!(t.snapshot().phase, Phase::YourTurn);
        t.play(Some(katastro::Point::new(3, 4))).unwrap();
        let queries = support::settle(&mut t, false, true);
        assert!(queries.iter().any(|q| q["maxVisits"] == 512));
        assert_eq!(
            t.snapshot().phase,
            Phase::Passed,
            "one-slot mode must still finish and grade"
        );
    }
}

#[test]
fn legacy_settings_and_completed_analysis_keep_the_historical_defaults_and_identity() {
    let temp = tempfile::tempdir().unwrap();
    let cfg = config(temp.path());
    let source = support::fixture(temp.path(), false);
    let mut review = Review::new(&temp.path().join("reviews"), &temp.path().join("cache")).unwrap();
    review.import(&source).unwrap();
    let mut historical = Engine::profile(&cfg).unwrap();
    historical.settings_digest = katastro::digest(b"logToStderr = true\nreportAnalysisWinratesAs = BLACK\nnumAnalysisThreads = 8\nnumSearchThreadsPerAnalysisThread = 1\nnnMaxBatchSize = 8\nnnCacheSizePowerOfTwo = 18\nnnMutexPoolSizePowerOfTwo = 14\nnumNNServerThreadsPerModel = 1\nmetalDeviceToUseThread0 = 0\nmaxVisits = 64\nanalysisPVLen = 1\n");
    review
        .store_analysis(
            &review.analysis_key(0, &historical).unwrap(),
            &Analysis {
                visits: 512,
                winrate: 0.5,
                score_lead: 1.0,
                suggestions: vec![],
                ownership: vec![0.0; 81],
                ownership_visits: 512,
            },
        )
        .unwrap();
    review.save_profile(&historical).unwrap();
    let db = rusqlite::Connection::open(temp.path().join("reviews")).unwrap();
    db.execute(
        "INSERT INTO settings(key,value) VALUES ('engine',?1)",
        [json!({"executable":cfg.executable,"model":cfg.model}).to_string()],
    )
    .unwrap();
    let c = Client::spawn(
        temp.path().join("reviews"),
        temp.path().join("cache"),
        EngineConfig::default(),
    );
    let restored = c.recv(Duration::from_secs(5)).unwrap();
    assert!(restored.error.is_none());
    assert_eq!(restored.config.analysis, AnalysisSettings::default());
    c.send(Command::Open(source)).unwrap();
    let s = until(&c, |s| s.document.is_some());
    assert_eq!(s.values[&0].visits, 512);
    assert_eq!(
        review
            .cached_analysis(&Engine::profile(&restored.config).unwrap())
            .unwrap()[&0]
            .visits,
        512
    );
}
