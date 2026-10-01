//! Cross-agent usage aggregation. Reports stay local and require no API keys.
use crate::error::{NurError, Result};

pub fn run(period: &str, home: Option<&str>, refresh_prices: bool) -> Result<serde_json::Value> {
    if !["today", "7", "month", "all"].contains(&period) {
        return Err(NurError::Other(
            "ledger period: today | 7 | month | all".into(),
        ));
    }
    crate::jev_local::ensure_bridge_script()?;
    let py = crate::jev_local::python()
        .ok_or_else(|| NurError::Other("usage ledger requires Python 3".into()))?;
    let script = crate::jev_local::home().join("usage_ledger.py");
    let mut args = vec![];
    if std::path::Path::new(&py)
        .file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("py"))
    {
        args.push("-3".to_string());
    }
    args.extend([
        script.to_string_lossy().to_string(),
        "--nur-home".into(),
        crate::config::nur_home().to_string_lossy().to_string(),
        "--period".into(),
        period.into(),
    ]);
    if let Some(home) = home {
        args.extend(["--home".into(), home.into()]);
    }
    if refresh_prices {
        args.push("--refresh-prices".into());
    }
    let refs: Vec<_> = args.iter().map(String::as_str).collect();
    let output = crate::ecosystem::run_capture(&py, &refs, None, 120_000).map_err(|_| {
        NurError::Other(
            "ledger could not read local usage logs; check the configured Python environment"
                .into(),
        )
    })?;
    serde_json::from_str(&output)
        .map_err(|_| NurError::Other("ledger helper returned no valid report".into()))
}

pub fn format(report: &serde_json::Value) -> String {
    if let Some(models) = report.get("models") {
        return std::format!(
            "Public price cache refreshed: {models} models. No credentials or usage were sent."
        );
    }
    let mut text = std::format!(
        "Local usage ledger - {}\n",
        report["period"].as_str().unwrap_or("month")
    );
    let mut reported = 0.;
    let mut estimated = 0.;
    let mut tokens = 0u64;
    let mut unpriced = 0u64;
    if let Some(rows) = report["groups"].as_array() {
        for row in rows {
            let count = ["input", "output", "cache_read", "cache_write"]
                .iter()
                .map(|k| row[k].as_u64().unwrap_or(0))
                .sum::<u64>();
            let charges = row["reported_usd"].as_f64().unwrap_or(0.);
            let estimate = row["estimated_usd"].as_f64().unwrap_or(0.);
            let missing = row["unpriced_requests"].as_u64().unwrap_or(0);
            reported += charges;
            estimated += estimate;
            tokens += count;
            unpriced += missing;
            text.push_str(&std::format!(
                "  {:<13} {:<38} {:>12} tokens  reported ${:.4}  estimate ${:.4}{}\n",
                row["agent"].as_str().unwrap_or("?"),
                row["model"].as_str().unwrap_or("?"),
                count,
                charges,
                estimate,
                if missing > 0 {
                    std::format!("  ({missing} unpriced)")
                } else {
                    String::new()
                }
            ));
        }
    }
    text.push_str(&std::format!(
        "\nTotal: {tokens} tokens | reported ${reported:.4} | API price estimate ${estimated:.4}\n"
    ));
    if tokens == 0 {
        text.push_str("No usage found in this period. Try `nur ledger --period all`.\n");
    }
    if unpriced > 0 {
        text.push_str("Unpriced usage is included in token totals. Run `nur ledger --refresh-prices` to fetch public rates.\n");
    }
    if let Some(warnings) = report["warnings"].as_object() {
        for (agent, count) in warnings {
            text.push_str(&std::format!(
                "{agent}: {count} unreadable source files were skipped.\n"
            ));
        }
    }
    text.push_str(report["note"].as_str().unwrap_or(""));
    text
}
