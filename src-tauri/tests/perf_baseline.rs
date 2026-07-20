//! PERF-11 / ROADMAP E1: reproducible headless search/filter/list cursor baseline.
//!
//! Default CI smoke seeds 1k tasks and asserts result schema + successful queries.
//! Scale to 10k with: `cargo test --test perf_baseline -- --ignored --nocapture`
//! Optional artifact dir: `VIBE_PERF_ARTIFACT_DIR` (writes `baseline-*.json`).

#![cfg(debug_assertions)]

use std::fs;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use sqlx::Row;
use tauri_app_lib::{
    commands::tasks::seed_scale_data,
    db::{self, TaskListQuery},
    models::ScaleStateDistribution,
};

const PAGE_SIZE: i64 = 100;
const DEFAULT_REPS: usize = 5;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Percentiles {
    p50_ms: f64,
    p95_ms: f64,
    samples_ms: Vec<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct QueryCaseResult {
    name: String,
    nav: String,
    search: String,
    sort_key: String,
    result_count: usize,
    has_more: bool,
    latency: Percentiles,
    explain_query_plan: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BaselineReport {
    schema_version: u32,
    scale: u32,
    distribution: ScaleStateDistribution,
    repetitions: usize,
    page_size: i64,
    seed_ms: f64,
    cases: Vec<QueryCaseResult>,
}

fn distribution_for_scale(total: u32) -> ScaleStateDistribution {
    // Deterministic 20/20/50/10 split matching docs/performance-baseline.md examples.
    match total {
        1_000 => ScaleStateDistribution {
            queued: 200,
            downloading: 200,
            completed: 500,
            failed: 100,
        },
        10_000 => ScaleStateDistribution {
            queued: 2_000,
            downloading: 2_000,
            completed: 5_000,
            failed: 1_000,
        },
        _ => panic!("unsupported scale {total}; use 1000 or 10000"),
    }
}

async fn test_pool(label: &str) -> sqlx::SqlitePool {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("vibe-db-perf-{label}-{id}.sqlite"));
    db::connect(&path).await.expect("connect").pool
}

fn base_query(nav: &str, search: &str, sort_key: &str) -> TaskListQuery {
    TaskListQuery {
        nav: nav.to_string(),
        search: search.to_string(),
        sort_key: sort_key.to_string(),
        sort_direction: "desc".to_string(),
        file_type: "all".to_string(),
        source: "all".to_string(),
        failure: "all".to_string(),
        resume: "all".to_string(),
        page: 0,
        page_size: PAGE_SIZE,
        cursor_value: None,
        cursor_id: None,
    }
}

fn percentile(sorted_ms: &[f64], p: f64) -> f64 {
    if sorted_ms.is_empty() {
        return 0.0;
    }
    let rank = ((p / 100.0) * (sorted_ms.len() as f64 - 1.0)).round() as usize;
    sorted_ms[rank.min(sorted_ms.len() - 1)]
}

fn percentiles(samples_ms: Vec<f64>) -> Percentiles {
    let mut sorted = samples_ms.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Percentiles {
        p50_ms: percentile(&sorted, 50.0),
        p95_ms: percentile(&sorted, 95.0),
        samples_ms,
    }
}

fn plan_details(rows: &[sqlx::sqlite::SqliteRow]) -> Vec<String> {
    rows.iter()
        .map(|row| {
            row.try_get::<String, _>("detail")
                .or_else(|_| row.try_get::<String, _>(3))
                .unwrap_or_default()
        })
        .collect()
}

async fn explain_search_plan(pool: &sqlx::SqlitePool, search: &str) -> Vec<String> {
    // Representative of PERF-01 three-field LOWER(...) LIKE '%term%' path.
    let pattern = format!("%{}%", search.to_ascii_lowercase());
    let rows = sqlx::query(
        r#"
        EXPLAIN QUERY PLAN
        SELECT id FROM tasks
        WHERE LOWER(file_name) LIKE ?1
           OR LOWER(source_key) LIKE ?1
           OR LOWER(url) LIKE ?1
        ORDER BY updated_at DESC, id ASC
        LIMIT 101
        "#,
    )
    .bind(&pattern)
    .fetch_all(pool)
    .await
    .expect("explain");
    plan_details(&rows)
}

async fn explain_list_all_plan(pool: &sqlx::SqlitePool) -> Vec<String> {
    let rows = sqlx::query(
        r#"
        EXPLAIN QUERY PLAN
        SELECT id FROM tasks
        ORDER BY updated_at DESC, id ASC
        LIMIT 101
        "#,
    )
    .fetch_all(pool)
    .await
    .expect("explain list all");
    plan_details(&rows)
}

async fn explain_status_plan(pool: &sqlx::SqlitePool, status: &str) -> Vec<String> {
    let rows = sqlx::query(
        r#"
        EXPLAIN QUERY PLAN
        SELECT id FROM tasks
        WHERE status = ?1
        ORDER BY updated_at DESC, id ASC
        LIMIT 101
        "#,
    )
    .bind(status)
    .fetch_all(pool)
    .await
    .expect("explain filter");
    plan_details(&rows)
}

async fn explain_failed_size_plan(pool: &sqlx::SqlitePool) -> Vec<String> {
    let rows = sqlx::query(
        r#"
        EXPLAIN QUERY PLAN
        SELECT id FROM tasks
        WHERE status = 'failed'
        ORDER BY total_size DESC, id ASC
        LIMIT 101
        "#,
    )
    .fetch_all(pool)
    .await
    .expect("explain failed size");
    plan_details(&rows)
}

async fn measure_case(
    pool: &sqlx::SqlitePool,
    name: &str,
    nav: &str,
    search: &str,
    sort_key: &str,
    reps: usize,
    explain: Vec<String>,
) -> QueryCaseResult {
    let mut samples_ms = Vec::with_capacity(reps);
    let mut last_count = 0usize;
    let mut last_has_more = false;

    for _ in 0..reps {
        let query = base_query(nav, search, sort_key);
        let started = Instant::now();
        let page = db::list_task_records_cursor(pool, &query)
            .await
            .expect("list_task_records_cursor");
        samples_ms.push(started.elapsed().as_secs_f64() * 1000.0);
        last_count = page.items.len();
        last_has_more = page.has_more;
    }

    QueryCaseResult {
        name: name.to_string(),
        nav: nav.to_string(),
        search: search.to_string(),
        sort_key: sort_key.to_string(),
        result_count: last_count,
        has_more: last_has_more,
        latency: percentiles(samples_ms),
        explain_query_plan: explain,
    }
}

async fn run_baseline(scale: u32, label: &str) -> BaselineReport {
    let reps = std::env::var("VIBE_PERF_REPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_REPS);
    let dist = distribution_for_scale(scale);
    let pool = test_pool(label).await;

    let seed_started = Instant::now();
    let seeded = seed_scale_data(&pool, &dist, true)
        .await
        .expect("seed_scale_data");
    assert_eq!(seeded, scale);
    let seed_ms = seed_started.elapsed().as_secs_f64() * 1000.0;

    // Warmup so the first measured sample is not dominated by cold page cache alone.
    let _ = db::list_task_records_cursor(&pool, &base_query("all", "", "updated_at"))
        .await
        .expect("warmup");

    let search_term = "scale-file-1";
    let search_plan = explain_search_plan(&pool, search_term).await;
    let list_plan = explain_list_all_plan(&pool).await;
    let filter_plan = explain_status_plan(&pool, "completed").await;
    let failed_plan = explain_failed_size_plan(&pool).await;

    let mut cases = Vec::new();
    cases.push(
        measure_case(
            &pool,
            "list_all_updated_at",
            "all",
            "",
            "updated_at",
            reps,
            list_plan,
        )
        .await,
    );
    cases.push(
        measure_case(
            &pool,
            "search_filename_prefix",
            "all",
            search_term,
            "updated_at",
            reps,
            search_plan,
        )
        .await,
    );
    cases.push(
        measure_case(
            &pool,
            "filter_completed",
            "completed",
            "",
            "updated_at",
            reps,
            filter_plan,
        )
        .await,
    );
    cases.push(
        measure_case(
            &pool,
            "filter_failed_sort_size",
            "failed",
            "",
            "file_size",
            reps,
            failed_plan,
        )
        .await,
    );

    BaselineReport {
        schema_version: 1,
        scale,
        distribution: dist,
        repetitions: reps,
        page_size: PAGE_SIZE,
        seed_ms,
        cases,
    }
}

fn assert_report_schema(report: &BaselineReport) {
    assert_eq!(report.schema_version, 1);
    assert!(report.repetitions >= 1);
    assert_eq!(report.page_size, PAGE_SIZE);
    assert_eq!(report.cases.len(), 4);
    for case in &report.cases {
        assert!(!case.name.is_empty());
        assert_eq!(case.latency.samples_ms.len(), report.repetitions);
        assert!(case.latency.p50_ms >= 0.0);
        assert!(case.latency.p95_ms >= case.latency.p50_ms);
        if case.search.is_empty() {
            assert!(
                case.result_count > 0,
                "expected non-empty page for {}",
                case.name
            );
        }
    }
}

fn maybe_write_artifact(report: &BaselineReport, filename: &str) {
    let Ok(dir) = std::env::var("VIBE_PERF_ARTIFACT_DIR") else {
        return;
    };
    let dir = PathBuf::from(dir);
    fs::create_dir_all(&dir).expect("create artifact dir");
    let path = dir.join(filename);
    let json = serde_json::to_string_pretty(report).expect("serialize report");
    fs::write(&path, json).expect("write baseline.json");
    eprintln!("PERF-11 wrote {}", path.display());
}

fn print_summary(report: &BaselineReport) {
    eprintln!(
        "PERF-11 scale={} seed_ms={:.1} reps={}",
        report.scale, report.seed_ms, report.repetitions
    );
    for case in &report.cases {
        eprintln!(
            "  {:<28} p50={:>8.2}ms p95={:>8.2}ms n={} has_more={} plan={:?}",
            case.name,
            case.latency.p50_ms,
            case.latency.p95_ms,
            case.result_count,
            case.has_more,
            case.explain_query_plan
        );
    }
}

#[tokio::test]
async fn perf_baseline_1k_smoke() {
    let report = run_baseline(1_000, "1k").await;
    assert_report_schema(&report);
    print_summary(&report);
    maybe_write_artifact(&report, "baseline-1k.json");

    // Soft smoke bound only: keep harness from silently regressing into multi-second stalls.
    // Absolute CI gates are deferred; record real numbers in performance-baseline-results.md.
    let list = report
        .cases
        .iter()
        .find(|c| c.name == "list_all_updated_at")
        .expect("list case");
    assert!(
        list.latency.p95_ms < 5_000.0,
        "1k list p95 unexpectedly high: {}ms",
        list.latency.p95_ms
    );
}

#[tokio::test]
#[ignore = "PERF-11 full 10k baseline; run via scripts/perf/run-baseline.ps1 -Include10k"]
async fn perf_baseline_10k() {
    let report = run_baseline(10_000, "10k").await;
    assert_report_schema(&report);
    print_summary(&report);
    maybe_write_artifact(&report, "baseline-10k.json");

    let list = report
        .cases
        .iter()
        .find(|c| c.name == "list_all_updated_at")
        .expect("list case");
    assert!(
        list.latency.p95_ms < 30_000.0,
        "10k list p95 unexpectedly high: {}ms",
        list.latency.p95_ms
    );
}

#[tokio::test]
async fn perf_baseline_report_serializes() {
    let dist = ScaleStateDistribution {
        queued: 1,
        downloading: 1,
        completed: 1,
        failed: 1,
    };
    let report = BaselineReport {
        schema_version: 1,
        scale: 4,
        distribution: dist,
        repetitions: 1,
        page_size: PAGE_SIZE,
        seed_ms: 1.0,
        cases: vec![QueryCaseResult {
            name: "list_all_updated_at".into(),
            nav: "all".into(),
            search: String::new(),
            sort_key: "updated_at".into(),
            result_count: 4,
            has_more: false,
            latency: Percentiles {
                p50_ms: 1.0,
                p95_ms: 1.0,
                samples_ms: vec![1.0],
            },
            explain_query_plan: vec!["SCAN tasks".into()],
        }],
    };
    let json = serde_json::to_value(&report).expect("json");
    assert_eq!(json["schemaVersion"], 1);
    assert!(json["cases"].as_array().unwrap()[0]["latency"]["p50Ms"].is_number());
}
