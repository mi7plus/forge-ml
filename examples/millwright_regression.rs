//! Millwright regression: predict a restaurant tip from the bill total and
//! party size with a linear model, and report held-out metrics.
//!
//! Run with:  cargo run --example millwright_regression

#[path = "support/data.rs"]
#[allow(dead_code)]
mod data;

use millwright::prelude::*;

fn main() -> Result<()> {
    let (headers, rows) = data::read_csv(data::dataset_path("tips.csv"));
    let bill = data::column(&headers, "total_bill");
    let size = data::column(&headers, "size");
    let tip = data::column(&headers, "tip");

    let mut features: Vec<Vec<f64>> = Vec::new();
    let mut target: Vec<f64> = Vec::new();
    for row in &rows {
        if let (Ok(b), Ok(s), Ok(t)) = (
            row[bill].parse::<f64>(),
            row[size].parse::<f64>(),
            row[tip].parse::<f64>(),
        ) {
            features.push(vec![b, s]);
            target.push(t);
        }
    }
    println!("Millwright · LinearRegression on tips.csv");
    println!("  {} complete rows (predict tip from total_bill, size)", features.len());

    // Deterministic 80/20 split (every 5th row is held out).
    let cols = vec!["total_bill".to_owned(), "size".to_owned()];
    let (mut tr_x, mut tr_y, mut te_x, mut te_y) = (vec![], vec![], vec![], vec![]);
    for (i, (row, &y)) in features.iter().zip(&target).enumerate() {
        if i % 5 == 0 {
            te_x.push(row.clone());
            te_y.push(y);
        } else {
            tr_x.push(row.clone());
            tr_y.push(y);
        }
    }

    let train = Dataset::new(Frame::from_rows(tr_x, cols.clone())?, tr_y)?;
    let test = Frame::from_rows(te_x, cols.clone())?;

    let mut model = LinearRegression::new();
    model.fit(&train)?;
    let predictions = model.predict(&test)?;

    let report = Report::for_task(&te_y, &predictions, Task::Regression);
    println!("  held-out rows: {}", te_y.len());
    for (name, value) in report.metrics() {
        println!("  {name:<6} = {value:.4}");
    }

    let probe = Frame::from_rows(vec![vec![25.0, 3.0]], cols)?;
    println!(
        "  predicted tip for a $25 bill, party of 3: ${:.2}",
        model.predict(&probe)?[0]
    );
    Ok(())
}
