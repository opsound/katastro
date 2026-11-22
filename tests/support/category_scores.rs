use katastro::{Analysis, EngineProfile, Review};
use std::path::Path;

pub fn profile() -> EngineProfile {
    EngineProfile {
        model_sha256: "example-model".into(),
        engine_sha256: "example-engine".into(),
        settings_digest: "example-black-perspective".into(),
    }
}

pub fn cache_scores(review: &mut Review, path: &Path, engine: &EngineProfile, scores: &[f64]) {
    review.import(path).unwrap();
    for (node, &score_lead) in review
        .document()
        .unwrap()
        .mainline
        .clone()
        .into_iter()
        .zip(scores)
    {
        review
            .store_analysis(
                &review.analysis_key(node, engine).unwrap(),
                &Analysis {
                    visits: 128,
                    score_lead,
                    winrate: 0.5,
                    suggestions: vec![],
                    ownership: vec![],
                    ownership_visits: 0,
                },
            )
            .unwrap();
    }
}
