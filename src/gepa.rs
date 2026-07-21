//! GEPA — reflective prompt optimisation over `nur bench`.
//!
//! Prompt tuning normally fails on infrastructure, not ideas: you need a
//! candidate space, an eval set, and a scorer that cannot be argued with.
//! `nur bench` already is the last two — it replays *your recorded tasks* in
//! isolated git worktrees and scores pass/fail from a real check command, plus
//! wall time and tokens. This module adds the first, and the search over it.
//!
//! The loop, per generation:
//!
//! 1. score every candidate instruction by replaying the task set through
//!    [`crate::bench::run_one`] — same worktrees, same check commands,
//! 2. keep the **Pareto front**: the candidates no other candidate beats on
//!    every objective at once (pass rate up, seconds down, tokens down). A
//!    single scalar would quietly trade away correctness for speed; a front
//!    keeps "slower but always passes" and "fast and usually passes" both
//!    alive,
//! 3. **reflect**: show the model a front member together with what actually
//!    happened — including the failures — and ask for a better instruction.
//!
//! This is the GEPA shape (Pareto frontier + reflective mutation) as described
//! by the Ax project, implemented natively rather than by taking on a
//! provider-layer dependency. See `docs/integrations-ax-graphjin.md`.
//!
//! Everything that decides *what to try next* is a pure function tested below;
//! only the replay and the mutation call touch the network.

use crate::api::ApiClient;
use crate::bench::{self, BenchResult, Task};
use crate::config::{muse_home, Config};
use crate::error::{MuseError, Result};
use crate::theme;
use std::path::PathBuf;

/// A candidate instruction prefix under optimisation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub id: usize,
    /// Prepended to every task prompt. Empty = the current baseline behaviour.
    pub instruction: String,
    /// Candidate this was reflected from, if any.
    pub parent: Option<usize>,
    pub generation: u32,
}

/// How a candidate did across the task set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Score {
    /// Fraction of tasks passed, 0.0..=1.0 — maximise.
    pub pass_rate: f64,
    /// Mean wall seconds per task — minimise.
    pub secs: f64,
    /// Mean tokens per task — minimise.
    pub tokens: f64,
}

impl Score {
    /// Fold per-task results into one score.
    pub fn from_results(results: &[BenchResult]) -> Self {
        if results.is_empty() {
            return Score {
                pass_rate: 0.0,
                secs: 0.0,
                tokens: 0.0,
            };
        }
        let n = results.len() as f64;
        Score {
            pass_rate: results.iter().filter(|r| r.passed).count() as f64 / n,
            secs: results.iter().map(|r| r.secs).sum::<f64>() / n,
            tokens: results.iter().map(|r| r.tokens as f64).sum::<f64>() / n,
        }
    }
}

/// Does `a` beat `b` on every objective, and strictly beat it on at least one?
///
/// Ties on an objective are allowed — otherwise nothing would ever dominate
/// anything, since equal token counts and equal pass rates are common.
pub fn dominates(a: &Score, b: &Score) -> bool {
    // Seconds are noisy; treat a sub-5% difference as a tie so timing jitter
    // cannot evict a candidate that is genuinely just as good.
    let secs_better = a.secs < b.secs * 0.95;
    let secs_worse = b.secs < a.secs * 0.95;

    let no_worse = a.pass_rate >= b.pass_rate && !secs_worse && a.tokens <= b.tokens;
    let strictly_better = a.pass_rate > b.pass_rate || secs_better || a.tokens < b.tokens;
    no_worse && strictly_better
}

/// Indices of the non-dominated candidates, in input order.
pub fn pareto_front(scores: &[Score]) -> Vec<usize> {
    (0..scores.len())
        .filter(|&i| {
            !scores
                .iter()
                .enumerate()
                .any(|(j, other)| j != i && dominates(other, &scores[i]))
        })
        .collect()
}

/// Pick `k` parents from the front, round-robin.
///
/// Round-robin rather than "best first" on purpose: the point of keeping a
/// front is to explore from all of it, and a scalar ranking to choose parents
/// would reintroduce exactly the collapse the front exists to prevent.
pub fn select_parents(front: &[usize], k: usize) -> Vec<usize> {
    if front.is_empty() || k == 0 {
        return Vec::new();
    }
    (0..k).map(|i| front[i % front.len()]).collect()
}

/// The single best candidate to report: highest pass rate, then cheapest, then
/// fastest. Used only to *present* a winner — never to steer the search.
pub fn best_index(scores: &[Score]) -> Option<usize> {
    (0..scores.len()).min_by(|&a, &b| {
        let (x, y) = (&scores[a], &scores[b]);
        y.pass_rate
            .partial_cmp(&x.pass_rate)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                x.tokens
                    .partial_cmp(&y.tokens)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then(
                x.secs
                    .partial_cmp(&y.secs)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    })
}

/// Prompt asking the model to improve one candidate, given real evidence.
///
/// The failures are the whole point — a mutation prompt without them is just
/// asking for synonyms.
pub fn mutation_prompt(
    candidate: &Candidate,
    score: &Score,
    failures: &[String],
    tasks: &[String],
) -> String {
    let mut s = String::from(
        "You are tuning the standing instruction prepended to a coding agent's task prompt.\n\n\
         # Current instruction\n",
    );
    if candidate.instruction.trim().is_empty() {
        s.push_str("(none — this is the untuned baseline)\n");
    } else {
        s.push_str(candidate.instruction.trim());
        s.push('\n');
    }
    s.push_str(&format!(
        "\n# Measured on {} recorded task(s)\n\
         pass rate: {:.0}%\nmean seconds: {:.1}\nmean tokens: {:.0}\n",
        tasks.len(),
        score.pass_rate * 100.0,
        score.secs,
        score.tokens
    ));
    s.push_str("\n# Tasks it must handle\n");
    for t in tasks {
        s.push_str(&format!("- {}\n", first_line(t)));
    }
    if failures.is_empty() {
        s.push_str(
            "\n# What went wrong\nNothing failed outright. Improve efficiency — fewer tokens \
             or less wandering — WITHOUT risking correctness.\n",
        );
    } else {
        s.push_str("\n# What went wrong\n");
        for f in failures {
            s.push_str(&format!("- {}\n", first_line(f)));
        }
    }
    s.push_str(
        "\n# Your reply\nReturn ONLY the replacement instruction text — no preamble, no \
         quotes, no markdown fences. It must be general enough to help every task listed \
         above, not overfitted to one. Keep it under 120 words. Prefer concrete operational \
         guidance (what to check, what order to work in, when to stop) over encouragement.",
    );
    s
}

fn first_line(s: &str) -> String {
    let line = s
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
    if line.chars().count() <= 200 {
        line.to_string()
    } else {
        line.chars().take(199).chain(['…']).collect()
    }
}

/// Strip the wrapping a model adds despite being told not to.
pub fn clean_instruction(raw: &str) -> String {
    let mut text = raw.trim();
    // Fenced block → take the inside.
    if text.starts_with("```") {
        if let Some(rest) = text.split_once('\n').map(|(_, r)| r) {
            text = rest
                .rsplit_once("```")
                .map(|(head, _)| head)
                .unwrap_or(rest);
        }
    }
    let text = text.trim();
    // Whole-string quoting.
    let text = text
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .unwrap_or(text);
    text.trim().to_string()
}

/// Scoreboard for one optimisation run.
pub fn optimize_report(
    label: &str,
    candidates: &[Candidate],
    scores: &[Score],
    front: &[usize],
) -> String {
    let mut out = format!("gepa · {label}\n");
    out.push_str(&format!(
        "  {:<4} {:<4} {:>7} {:>8} {:>9}  {}\n",
        "cand", "gen", "pass", "seconds", "tokens", "instruction"
    ));
    for (i, c) in candidates.iter().enumerate() {
        let s = scores[i];
        let mark = if front.contains(&i) { "◆" } else { " " };
        let text = if c.instruction.trim().is_empty() {
            "(baseline)".to_string()
        } else {
            first_line(&c.instruction).chars().take(60).collect()
        };
        out.push_str(&format!(
            "{mark} #{:<3} {:<4} {:>6.0}% {:>8.1} {:>9.0}  {}\n",
            c.id,
            c.generation,
            s.pass_rate * 100.0,
            s.secs,
            s.tokens,
            text
        ));
    }
    out.push_str("  ◆ = Pareto front (nothing beats it on every objective at once)\n");
    out
}

/// Where a winning instruction is saved.
pub fn optimized_path(label: &str) -> PathBuf {
    let clean: String = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    muse_home()
        .join("bench")
        .join("optimized")
        .join(format!("{clean}.md"))
}

// ── driver ───────────────────────────────────────────────────────────────

/// Run the optimiser. `name` is a bench task or "all".
pub async fn run_optimize(
    name: &str,
    generations: u32,
    population: usize,
    client: ApiClient,
    cfg: Config,
    cwd: PathBuf,
) -> Result<()> {
    if !bench::is_git_repo_pub(&cwd) {
        return Err(MuseError::Other(
            "gepa needs a git repo (it replays in isolated worktrees) — run inside your project"
                .into(),
        ));
    }
    let tasks: Vec<Task> = if name == "all" {
        bench::list_tasks_pub()
    } else {
        vec![bench::load_task(name).ok_or_else(|| {
            MuseError::Other(format!("no bench task `{name}` — see nur bench list"))
        })?]
    };
    if tasks.is_empty() {
        return Err(MuseError::Other(
            "no bench tasks recorded — record one first: nur bench add <name> \"<prompt>\" --check \"<cmd>\"".into(),
        ));
    }
    let population = population.clamp(2, 8);
    let generations = generations.clamp(1, 10);
    let runs = tasks.len() * population * generations as usize;
    theme::print_info(&format!(
        "gepa · {} task(s) · {population} candidates × {generations} generation(s) = {runs} agent run(s)",
        tasks.len()
    ));
    theme::print_info("each run is a real agent turn in a throwaway worktree — this costs tokens.");

    let task_texts: Vec<String> = tasks.iter().map(|t| t.prompt.clone()).collect();
    let mut candidates: Vec<Candidate> = vec![Candidate {
        id: 0,
        instruction: String::new(),
        parent: None,
        generation: 0,
    }];
    let mut scores: Vec<Score> = Vec::new();
    let mut failures: Vec<Vec<String>> = Vec::new();
    let mut next_id = 1usize;

    for gen in 0..generations {
        // Score everything not yet scored.
        while scores.len() < candidates.len() {
            let i = scores.len();
            let c = &candidates[i];
            theme::print_info(&format!(
                "→ gen {gen} · candidate #{} · {}",
                c.id,
                if c.instruction.is_empty() {
                    "baseline".into()
                } else {
                    first_line(&c.instruction)
                }
            ));
            let mut results = Vec::new();
            let mut fails = Vec::new();
            for task in &tasks {
                let r = bench::run_one(task, &cfg.model, &client, &cfg, &cwd, &c.instruction).await;
                match r {
                    Ok(r) => {
                        if !r.passed {
                            fails.push(format!(
                                "task `{}` failed{}",
                                task.name,
                                r.error
                                    .as_deref()
                                    .map(|e| format!(": {e}"))
                                    .unwrap_or_default()
                            ));
                        }
                        results.push(r);
                    }
                    Err(e) => {
                        fails.push(format!("task `{}` could not run: {e}", task.name));
                        results.push(BenchResult {
                            model: cfg.model.clone(),
                            passed: false,
                            secs: 0.0,
                            tokens: 0,
                            error: Some(e.to_string()),
                        });
                    }
                }
            }
            scores.push(Score::from_results(&results));
            failures.push(fails);
        }

        let front = pareto_front(&scores);
        print!("{}", optimize_report(name, &candidates, &scores, &front));

        if gen + 1 == generations {
            break;
        }

        // Reflect: ask for one improved instruction per open slot.
        let parents = select_parents(&front, population.saturating_sub(1));
        for p in parents {
            let prompt = mutation_prompt(&candidates[p], &scores[p], &failures[p], &task_texts);
            let req = crate::api::fusion::question_request(&cfg.model, &prompt);
            let proposal = match client.create_response(&req).await {
                Ok(resp) => clean_instruction(&resp.output_text()),
                Err(e) => {
                    theme::print_info(&format!("  reflection failed: {e}"));
                    continue;
                }
            };
            if proposal.is_empty() || candidates.iter().any(|c| c.instruction == proposal) {
                continue; // empty or already tried
            }
            candidates.push(Candidate {
                id: next_id,
                instruction: proposal,
                parent: Some(candidates[p].id),
                generation: gen + 1,
            });
            next_id += 1;
        }
    }

    let front = pareto_front(&scores);
    let Some(best) = best_index(&scores) else {
        return Err(MuseError::Other(
            "gepa produced no scored candidates".into(),
        ));
    };
    let winner = &candidates[best];
    print!("{}", optimize_report(name, &candidates, &scores, &front));

    if winner.instruction.trim().is_empty() {
        theme::print_info(
            "the untuned baseline won — no proposed instruction beat it. That is a result: \
             keep the prompt as it is.",
        );
        return Ok(());
    }

    let path = optimized_path(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let s = &scores[best];
    std::fs::write(
        &path,
        format!(
            "# gepa · {name}\n\n\
             pass {:.0}% · {:.1}s · {:.0} tokens (mean over {} task(s), model {})\n\n\
             ---\n\n{}\n",
            s.pass_rate * 100.0,
            s.secs,
            s.tokens,
            tasks.len(),
            cfg.model,
            winner.instruction.trim()
        ),
    )?;
    theme::print_ok(&format!("winner saved: {}", path.display()));
    theme::print_info("use it with /bro-style riders, NUR.md, or a sticky skill — nur does not apply it automatically.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score(pass: f64, secs: f64, tokens: f64) -> Score {
        Score {
            pass_rate: pass,
            secs,
            tokens,
        }
    }

    fn cand(id: usize, instruction: &str) -> Candidate {
        Candidate {
            id,
            instruction: instruction.into(),
            parent: None,
            generation: 0,
        }
    }

    #[test]
    fn score_averages_over_the_task_set() {
        let results = vec![
            BenchResult {
                model: "m".into(),
                passed: true,
                secs: 10.0,
                tokens: 100,
                error: None,
            },
            BenchResult {
                model: "m".into(),
                passed: false,
                secs: 20.0,
                tokens: 300,
                error: None,
            },
        ];
        let s = Score::from_results(&results);
        assert_eq!(s.pass_rate, 0.5);
        assert_eq!(s.secs, 15.0);
        assert_eq!(s.tokens, 200.0);
        // No results is a zero score, not a divide by zero.
        assert_eq!(Score::from_results(&[]).pass_rate, 0.0);
    }

    #[test]
    fn dominance_requires_being_no_worse_everywhere() {
        let good = score(1.0, 10.0, 100.0);
        let worse_all = score(0.5, 20.0, 200.0);
        assert!(dominates(&good, &worse_all));
        assert!(!dominates(&worse_all, &good));

        // Better pass rate but far more tokens — neither dominates.
        let accurate = score(1.0, 10.0, 900.0);
        let cheap = score(0.8, 10.0, 100.0);
        assert!(!dominates(&accurate, &cheap));
        assert!(!dominates(&cheap, &accurate));

        // Identical scores never dominate each other.
        assert!(!dominates(&good, &good));
    }

    #[test]
    fn timing_jitter_alone_does_not_create_dominance() {
        // 2% faster, everything else equal — noise, not an improvement.
        let a = score(1.0, 10.0, 100.0);
        let b = score(1.0, 10.2, 100.0);
        assert!(!dominates(&a, &b), "sub-threshold timing must not dominate");
        // 20% faster is a real difference.
        let c = score(1.0, 8.0, 100.0);
        assert!(dominates(&c, &b));
    }

    #[test]
    fn the_front_keeps_every_incomparable_tradeoff() {
        let scores = vec![
            score(1.0, 30.0, 900.0), // accurate, slow, expensive
            score(0.8, 5.0, 100.0),  // fast and cheap, less accurate
            score(0.5, 40.0, 950.0), // dominated by #0 on every axis
            score(1.0, 30.0, 400.0), // dominates #0 (same pass/secs, fewer tokens)
        ];
        let front = pareto_front(&scores);
        assert!(front.contains(&1), "the cheap tradeoff must survive");
        assert!(front.contains(&3), "the improved accurate one must survive");
        assert!(!front.contains(&0), "#0 is dominated by #3");
        assert!(!front.contains(&2), "#2 is dominated outright");
    }

    #[test]
    fn a_single_candidate_is_its_own_front() {
        assert_eq!(pareto_front(&[score(0.0, 1.0, 1.0)]), vec![0]);
        assert!(pareto_front(&[]).is_empty());
    }

    #[test]
    fn parents_are_drawn_round_robin_across_the_whole_front() {
        assert_eq!(select_parents(&[2, 5], 5), vec![2, 5, 2, 5, 2]);
        assert!(select_parents(&[], 3).is_empty());
        assert!(select_parents(&[1], 0).is_empty());
    }

    #[test]
    fn the_reported_winner_prefers_correctness_then_cost() {
        let scores = vec![
            score(0.9, 1.0, 10.0),   // fastest + cheapest, but not the best pass rate
            score(1.0, 50.0, 900.0), // passes everything
            score(1.0, 50.0, 500.0), // passes everything, cheaper
        ];
        assert_eq!(best_index(&scores), Some(2));
    }

    #[test]
    fn the_mutation_prompt_carries_the_evidence() {
        let c = cand(3, "Read tests before editing.");
        let s = score(0.5, 12.0, 400.0);
        let p = mutation_prompt(
            &c,
            &s,
            &["task `auth` failed: assertion left != right".into()],
            &["fix the failing auth test".into()],
        );
        assert!(p.contains("Read tests before editing."), "current text");
        assert!(p.contains("50%"), "measured pass rate");
        assert!(p.contains("assertion left != right"), "the actual failure");
        assert!(
            p.contains("fix the failing auth test"),
            "what it must handle"
        );
        assert!(
            p.contains("ONLY the replacement instruction"),
            "output contract"
        );

        // Baseline is labelled, not left blank and confusing.
        let base = mutation_prompt(&cand(0, ""), &s, &[], &["t".into()]);
        assert!(base.contains("untuned baseline"));
        assert!(base.contains("Nothing failed outright"));
    }

    #[test]
    fn model_wrapping_is_stripped_from_proposals() {
        assert_eq!(clean_instruction("  Do the thing.  "), "Do the thing.");
        assert_eq!(
            clean_instruction("```\nDo the thing.\n```"),
            "Do the thing."
        );
        assert_eq!(
            clean_instruction("```text\nDo the thing.\n```"),
            "Do the thing."
        );
        assert_eq!(clean_instruction("\"Do the thing.\""), "Do the thing.");
        assert_eq!(clean_instruction(""), "");
    }

    #[test]
    fn the_report_marks_the_front_and_names_the_baseline() {
        let candidates = vec![cand(0, ""), cand(1, "Check the tests first.")];
        let scores = vec![score(0.5, 10.0, 100.0), score(1.0, 10.0, 100.0)];
        let front = pareto_front(&scores);
        let out = optimize_report("auth", &candidates, &scores, &front);
        assert!(out.contains("(baseline)"));
        assert!(out.contains("Check the tests first."));
        // Candidate 1 dominates 0, so only 1 is marked.
        let marked: Vec<&str> = out.lines().filter(|l| l.starts_with('◆')).collect();
        assert_eq!(marked.len(), 1, "got {marked:?}");
        assert!(marked[0].contains("#1"));
    }

    /// A task name reaches the filesystem, so traversal must not survive it.
    #[test]
    fn optimized_path_is_filesystem_safe() {
        let p = optimized_path("all/../etc");
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        assert_eq!(name, "all----etc.md", "separators and dots are neutralised");
        assert_eq!(
            p.parent().unwrap(),
            muse_home().join("bench").join("optimized"),
            "the file must land inside the optimized dir, never above it"
        );
        assert_eq!(optimized_path("auth").file_name().unwrap(), "auth.md");
    }
}
