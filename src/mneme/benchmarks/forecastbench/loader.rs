//! Load + join ForecastBench question_set / resolution_set JSON files.
//!
//! Phase 1: market questions only (one question → one resolution). Dataset
//! questions are skipped (see `mod.rs`).

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use chrono::{DateTime, Utc};
use thiserror::Error;

use super::types::{
    is_market_source, FBQuestion, FBQuestionId, FBQuestionSet, FBResolution, FBResolutionSet,
    MarketQuestionWithResolution,
};

#[derive(Debug, Error)]
pub enum LoaderError {
    #[error("read {path}: {source}")]
    Io { path: String, source: std::io::Error },
    #[error("parse {path}: {source}")]
    Json { path: String, source: serde_json::Error },
}

pub fn load_question_set(path: &Path) -> Result<FBQuestionSet, LoaderError> {
    let bytes = fs::read(path).map_err(|source| LoaderError::Io {
        path: path.display().to_string(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| LoaderError::Json {
        path: path.display().to_string(),
        source,
    })
}

pub fn load_resolution_set(path: &Path) -> Result<FBResolutionSet, LoaderError> {
    let bytes = fs::read(path).map_err(|source| LoaderError::Io {
        path: path.display().to_string(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| LoaderError::Json {
        path: path.display().to_string(),
        source,
    })
}

/// Pair each (single-id) market question with its single resolution.
/// Drops questions that:
/// - Aren't from a market source (manifold/metaculus/polymarket/infer)
/// - Are combination questions (id is an array — out of scope for Phase 1)
/// - Have no matching resolution in the supplied resolution set
///
/// Returns `Vec<MarketQuestionWithResolution>` sorted by `resolution_datetime_utc`
/// ascending so the caller can take the earliest-resolved N for incremental
/// runs.
pub fn join_market_questions(
    questions: &[FBQuestion],
    resolutions: &[FBResolution],
) -> Vec<MarketQuestionWithResolution> {
    // Index single-id market resolutions by id.
    let mut by_id: HashMap<&str, &FBResolution> = HashMap::with_capacity(resolutions.len());
    for r in resolutions {
        if !is_market_source(&r.source) {
            continue;
        }
        if let FBQuestionId::Single(id) = &r.id {
            by_id.insert(id.as_str(), r);
        }
    }

    let mut joined = Vec::new();
    for q in questions {
        if !is_market_source(&q.source) {
            continue;
        }
        let Some(qid) = q.id.as_single() else {
            continue; // skip combination questions
        };
        let Some(res) = by_id.get(qid) else {
            continue; // unresolved / not in this resolution_set release
        };
        let res_datetime = match parse_resolution_date(&res.resolution_date) {
            Some(dt) => dt,
            None => continue,
        };
        joined.push(MarketQuestionWithResolution {
            question: q.clone(),
            resolution: (*res).clone(),
            resolution_datetime_utc: res_datetime,
        });
    }
    joined.sort_by_key(|m| m.resolution_datetime_utc);
    joined
}

fn parse_resolution_date(s: &str) -> Option<DateTime<Utc>> {
    // Accept "YYYY-MM-DD" (most common) or full RFC 3339.
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    let date = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()?;
    let datetime = date.and_hms_opt(23, 59, 59)?;
    Some(DateTime::<Utc>::from_naive_utc_and_offset(datetime, Utc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn write_file(dir: &TempDir, name: &str, contents: &str) -> std::path::PathBuf {
        let p = dir.path().join(name);
        let mut f = fs::File::create(&p).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        p
    }

    #[test]
    fn load_minimal_question_set() {
        let dir = TempDir::new().unwrap();
        let p = write_file(
            &dir,
            "qs.json",
            r#"{
                "forecast_due_date": "2024-07-21",
                "question_set": "test",
                "questions": [{
                    "id": "q1",
                    "source": "manifold",
                    "question": "Will X?",
                    "resolution_criteria": "...",
                    "url": "https://e.x",
                    "freeze_datetime": "2024-07-12T00:00:00+00:00",
                    "freeze_datetime_value": "0.5",
                    "freeze_datetime_value_explanation": "..."
                }]
            }"#,
        );
        let qs = load_question_set(&p).unwrap();
        assert_eq!(qs.questions.len(), 1);
        assert_eq!(qs.questions[0].id.as_single(), Some("q1"));
    }

    #[test]
    fn join_drops_unresolved_questions() {
        let q1 = sample_question("q1", "manifold");
        let q2 = sample_question("q2", "manifold");
        let r2 = sample_resolution("q2", "manifold", "2024-12-31", 1.0);
        let joined = join_market_questions(&[q1, q2], &[r2]);
        assert_eq!(joined.len(), 1);
        assert_eq!(joined[0].question.id.as_single(), Some("q2"));
        assert_eq!(joined[0].resolution.resolved_to, 1.0);
    }

    #[test]
    fn join_skips_dataset_questions() {
        let q_market = sample_question("q1", "manifold");
        let q_dataset = sample_question("q2", "fred");
        let r1 = sample_resolution("q1", "manifold", "2024-12-31", 1.0);
        let r2 = sample_resolution("q2", "fred", "2024-12-31", 0.0);
        let joined = join_market_questions(&[q_market, q_dataset], &[r1, r2]);
        assert_eq!(joined.len(), 1);
        assert_eq!(joined[0].question.source, "manifold");
    }

    #[test]
    fn join_sorts_by_resolution_datetime() {
        let q_a = sample_question("a", "manifold");
        let q_b = sample_question("b", "metaculus");
        let q_c = sample_question("c", "polymarket");
        let r_a = sample_resolution("a", "manifold", "2024-12-31", 1.0);
        let r_b = sample_resolution("b", "metaculus", "2024-08-01", 0.0);
        let r_c = sample_resolution("c", "polymarket", "2024-10-15", 1.0);
        let joined = join_market_questions(&[q_a, q_b, q_c], &[r_a, r_b, r_c]);
        assert_eq!(joined[0].question.id.as_single(), Some("b")); // 2024-08-01
        assert_eq!(joined[1].question.id.as_single(), Some("c")); // 2024-10-15
        assert_eq!(joined[2].question.id.as_single(), Some("a")); // 2024-12-31
    }

    fn sample_question(id: &str, source: &str) -> FBQuestion {
        FBQuestion {
            id: FBQuestionId::Single(id.to_string()),
            source: source.to_string(),
            question: format!("Will {}?", id),
            resolution_criteria: "...".to_string(),
            background: None,
            url: "https://e.x".to_string(),
            freeze_datetime: "2024-07-12T00:00:00+00:00".to_string(),
            freeze_datetime_value: "0.5".to_string(),
            freeze_datetime_value_explanation: "...".to_string(),
            market_info_resolution_datetime: None,
            forecast_horizons: vec![],
        }
    }

    fn sample_resolution(id: &str, source: &str, date: &str, resolved_to: f64) -> FBResolution {
        FBResolution {
            id: FBQuestionId::Single(id.to_string()),
            source: source.to_string(),
            direction: None,
            resolution_date: date.to_string(),
            resolved_to,
            resolved: true,
        }
    }

    /// Smoke test against the vendored 2024-07-21 release. Skipped by default
    /// (requires the dataset under `programs/_benchmarks/forecastbench/`).
    /// Run with `cargo test --lib --ignored vendored_smoke -- --nocapture`.
    #[test]
    #[ignore = "requires vendored ForecastBench data"]
    fn vendored_smoke() {
        let qs = load_question_set(Path::new(
            "programs/_benchmarks/forecastbench/2024-07-21-llm.json",
        ))
        .expect("question set");
        let rs = load_resolution_set(Path::new(
            "programs/_benchmarks/forecastbench/2024-07-21_resolution_set.json",
        ))
        .expect("resolution set");
        assert!(!qs.questions.is_empty(), "expected questions");
        assert!(!rs.resolutions.is_empty(), "expected resolutions");
        let joined = join_market_questions(&qs.questions, &rs.resolutions);
        eprintln!(
            "vendored 2024-07-21: {} questions in set, {} resolutions, {} joined market questions",
            qs.questions.len(),
            rs.resolutions.len(),
            joined.len()
        );
        // Source breakdown of joined.
        use std::collections::BTreeMap;
        let mut by_src: BTreeMap<&str, usize> = BTreeMap::new();
        for j in &joined {
            *by_src.entry(j.question.source.as_str()).or_insert(0) += 1;
        }
        eprintln!("joined source breakdown:");
        for (s, c) in &by_src {
            eprintln!("  {}: {}", s, c);
        }
        // Base rate.
        let yes = joined.iter().filter(|j| j.resolution.resolved_to >= 0.5).count();
        eprintln!(
            "base rate: {}/{} YES ({:.1}%)",
            yes,
            joined.len(),
            100.0 * yes as f64 / joined.len() as f64
        );
        if let (Some(first), Some(last)) = (joined.first(), joined.last()) {
            eprintln!(
                "resolution date range: {} → {}",
                first.resolution_datetime_utc.format("%Y-%m-%d"),
                last.resolution_datetime_utc.format("%Y-%m-%d")
            );
        }
        assert!(
            joined.len() >= 100,
            "expected ≥100 resolved market questions, got {}",
            joined.len()
        );
        for w in joined.windows(2) {
            assert!(w[0].resolution_datetime_utc <= w[1].resolution_datetime_utc);
        }
    }

    /// End-to-end: load vendored data, run the crowd-baseline forecaster
    /// (predict the freeze-cutoff market price for each question), report
    /// Brier Index. This is the bar mneme has to beat.
    #[tokio::test]
    #[ignore = "requires vendored ForecastBench data"]
    async fn vendored_crowd_baseline_brier_index() {
        use crate::mneme::benchmarks::forecastbench::{freeze_value_forecaster, run_backtest};

        let qs = load_question_set(Path::new(
            "programs/_benchmarks/forecastbench/2024-07-21-llm.json",
        ))
        .unwrap();
        let rs = load_resolution_set(Path::new(
            "programs/_benchmarks/forecastbench/2024-07-21_resolution_set.json",
        ))
        .unwrap();
        let joined = join_market_questions(&qs.questions, &rs.resolutions);
        let r = run_backtest(&joined, None, freeze_value_forecaster()).await;
        eprintln!(
            "crowd baseline (freeze_datetime_value) on {} resolved questions:",
            r.predictions.len()
        );
        eprintln!("  failures: {}", r.failures.len());
        eprintln!("  mean Brier: {:.4}", r.mean_brier.unwrap_or(f64::NAN));
        eprintln!("  Brier Index: {:.2}", r.brier_index.unwrap_or(f64::NAN));
        if let Some(ci) = r.ci_95 {
            eprintln!(
                "  95% CI on mean Brier: [{:.4}, {:.4}]",
                ci.lower, ci.upper
            );
        }
        // Sanity: the crowd should be substantially better than coin flip on
        // ForecastBench markets — Brier Index should be positive.
        assert!(
            r.brier_index.unwrap() > 0.0,
            "crowd baseline should beat coin flip; got BI={}",
            r.brier_index.unwrap()
        );
    }
}
