//! `meat-proxy --judge-eval`: how the model judges your own pull requests, printed.

use crate::*;

/// `meat-proxy --judge-eval`: fetch the real pull requests (read only), judge them the way the app
/// does, and print what each would get. Point MEAT_PROXY_DB at a copy of the database.
pub(crate) fn judge_eval() {
    // It sends what the app would: only with the feature shown and AI sorting switched on.
    if !judge::feature_enabled() {
        return eprintln!("The AI feature is hidden: run with MEAT_PROXY_AI=1.");
    }
    if !Database::open_default()
        .and_then(|db| db.load_snapshot())
        .is_ok_and(|snap| snap.ai_enabled)
    {
        return eprintln!("AI sorting is off: switch it on in Settings › AI first.");
    }
    let Some(mut cfg) = judge::JudgeConfig::load() else {
        eprintln!("No judge config: put an api_key in ~/.config/meat-proxy/judge.json");
        return;
    };
    cfg.max_per_sync = 100;
    let layout = sections::Layout::default();
    let questions = layout.needed_questions();
    println!(
        "{} sections; questions asked: {}",
        layout.sections.len(),
        questions
            .iter()
            .map(|q| q.id.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    let snap = match Database::open_default().and_then(|db| db.load_snapshot()) {
        Ok(snap) => snap,
        Err(error) => return eprintln!("Could not read the saved data: {error}"),
    };
    let started = std::time::Instant::now();
    let out = match github::fetch_live(&snap.repos, &snap.rules, snap.sync_options) {
        Ok(out) => out,
        Err(error) => return eprintln!("Sync failed: {error}"),
    };
    println!(
        "synced {} pull requests in {:.1}s; model {} via {}",
        out.prs.len(),
        started.elapsed().as_secs_f32(),
        cfg.model,
        cfg.base_url
    );
    let now = current_epoch_hour();
    let attention =
        |pr: &PullRequest| pr.attention(snap.draft_stale_hours, snap.open_stale_hours, now);
    let mut cache = std::collections::HashMap::new();
    let plan = judge::plan(&out.prs, attention, &cache, &cfg, &questions);
    let candidates = plan.cached + plan.settled.len() + plan.to_ask.len() + plan.deferred;
    println!(
        "{} need a judgment: {} answered by rules, {} for the model ({} deferred by the cap)",
        candidates,
        plan.settled.len(),
        plan.to_ask.len(),
        plan.deferred
    );
    cache.extend(plan.settled);
    let asked = plan.to_ask.len();
    let started = std::time::Instant::now();
    let batch = judge::run_batch(&cfg, &questions, plan.to_ask, 0.0, &Default::default());
    println!(
        "{} calls in {:.1}s ({} answered): {} prompt + {} completion tokens, ${:.5}{}",
        asked,
        started.elapsed().as_secs_f32(),
        batch.judged.len(),
        batch.usage.prompt_tokens,
        batch.usage.completion_tokens,
        batch.usage.cost_usd,
        if batch.fatal { "  STOPPED EARLY" } else { "" }
    );
    for error in batch.errors.iter().take(5) {
        println!("  error: {error}");
    }
    cache.extend(batch.judged);

    let again = judge::plan(&out.prs, attention, &cache, &cfg, &questions);
    println!(
        "second pass: {} calls, {} answered from the cache\n",
        again.to_ask.len(),
        again.cached
    );

    let mut rows: Vec<_> = out
        .prs
        .iter()
        .filter_map(|pr| cache.get(&pr.id).map(|j| (pr, j)))
        .map(|(pr, j)| {
            let at = layout.section_of(&sections::Subject {
                pr,
                attention: attention(pr),
                answers: &j.answers,
            });
            (at, pr, j)
        })
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.id.cmp(&b.1.id)));
    println!(
        "{:<18} {:<28} {:<4} {:<14} title",
        "section", "answers", "ext", "author"
    );
    for (at, pr, j) in rows {
        let section = at.map_or("(none)", |i| layout.sections[i].name.as_str());
        let mut answers: Vec<_> = j.answers.iter().collect();
        answers.sort_by(|a, b| a.0.cmp(b.0));
        let odds: Vec<String> = answers
            .iter()
            .map(|(id, p)| format!("{}{:.0}", id.chars().next().unwrap_or('?'), **p * 100.0))
            .collect();
        let title: String = pr.title.chars().take(64).collect();
        println!(
            "{:<18} {:<28} {:<4} {:<14} {}   [{}]",
            section,
            odds.join(" "),
            if pr.external { "ext" } else { "" },
            pr.author.chars().take(14).collect::<String>(),
            title,
            j.by
        );
    }
}
