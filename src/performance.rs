use std::time::Instant;

pub struct BudgetResult {
    pub name: &'static str,
    pub elapsed_ms: u128,
    pub budget_ms: u128,
    pub passed: bool,
}
pub fn run() -> Vec<BudgetResult> {
    vec![
        measure("Notebook parse (10k cells)", 250, || {
            let source = (0..10_000)
                .map(|i| format!("//# %%\nlet x_{i} = {i};\n"))
                .collect::<String>();
            std::hint::black_box(crate::notebook::NotebookDocument::parse_rust(&source));
        }),
        measure("Table filter/sort (100k rows)", 350, || {
            let mut rows = (0..100_000)
                .rev()
                .map(|i| format!("sample-{i:06}"))
                .filter(|v| v.contains('5'))
                .collect::<Vec<_>>();
            rows.sort();
            std::hint::black_box(rows);
        }),
        measure("Plot preparation (1m points)", 500, || {
            let points = (0..1_000_000)
                .map(|i| [i as f64, (i as f64 / 100.0).sin()])
                .collect::<Vec<_>>();
            std::hint::black_box(points);
        }),
    ]
}
fn measure(name: &'static str, budget_ms: u128, operation: impl FnOnce()) -> BudgetResult {
    let start = Instant::now();
    operation();
    let elapsed_ms = start.elapsed().as_millis();
    BudgetResult {
        name,
        elapsed_ms,
        budget_ms,
        passed: elapsed_ms <= budget_ms,
    }
}
pub fn report(results: &[BudgetResult]) -> String {
    results
        .iter()
        .map(|r| {
            format!(
                "{}  {} ms / {} ms  {}",
                r.name,
                r.elapsed_ms,
                r.budget_ms,
                if r.passed { "PASS" } else { "OVER BUDGET" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn report_has_all_budgets() {
        let results = run();
        assert_eq!(results.len(), 3);
        assert!(report(&results).contains("Notebook parse"));
    }
}
