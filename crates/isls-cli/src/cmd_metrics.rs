// isls-cli/src/cmd_metrics.rs — D7/W4: Generation Metrics CLI
//
// isls metrics              — Summary of recent generations
// isls metrics --compare    — CLI vs Cockpit comparison table
// isls metrics --last N     — Last N generation entries

use isls_forge_llm::metrics;

/// Default summary: show recent metrics.
pub fn cmd_metrics_summary() {
    let all = metrics::load_metrics();
    println!("{}", metrics::format_summary(&all));
}

/// Show CLI vs Cockpit comparison table.
pub fn cmd_metrics_compare() {
    let all = metrics::load_metrics();
    if all.is_empty() {
        println!("No generation metrics recorded yet.");
        println!("Run 'isls forge-chat' or use the Cockpit to generate some apps first.");
        return;
    }
    let table = metrics::compare_metrics(&all);
    println!("{}", metrics::format_comparison(&table));
}

/// Show last N generation entries.
pub fn cmd_metrics_last(n: usize) {
    let last = metrics::load_last_n(n);
    if last.is_empty() {
        println!("No generation metrics recorded yet.");
        return;
    }
    println!("ISLS Generation Metrics — last {} entries", last.len());
    println!("─────────────────────────────────────────────────────");
    for m in &last {
        println!();
        println!("  ID:          {}", m.id);
        println!("  Date:        {}", &m.timestamp[..19]);
        println!("  Source:      {}", m.source);
        println!("  Description: {}", m.description);
        println!("  Entities:    {}", m.entity_count);
        println!("  Files:       {} (structural: {}, LLM: {})", m.file_count, m.structural_files, m.llm_files);
        println!("  Tokens:      {}", m.total_tokens);
        println!("  Compile:     {}", if m.compile_success { "success" } else { "failed" });
        println!("  Coagula:     {} cycles", m.coagula_cycles);
        println!("  Duration:    {:.1}s", m.duration_secs);
        println!("  Turns:       {}", m.conversation_turns);
        if !m.norms_activated.is_empty() {
            println!("  Norms:       {}", m.norms_activated.join(", "));
        }
    }
}
