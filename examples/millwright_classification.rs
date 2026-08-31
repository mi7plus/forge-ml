//! Millwright classification: predict the iris species from its four flower
//! measurements with a random forest, and report held-out accuracy.
//!
//! (Iris has three classes, so this uses RandomForest — Millwright's
//! LogisticRegression is binary-only.)
//!
//! Run with:  cargo run --example millwright_classification

#[path = "support/data.rs"]
#[allow(dead_code)]
mod data;

use millwright::prelude::*;

const FEATURES: [&str; 4] = [
    "sepal_length",
    "sepal_width",
    "petal_length",
    "petal_width",
];

fn main() -> Result<()> {
    let (headers, rows) = data::read_csv(data::dataset_path("iris.csv"));
    let feature_idx: Vec<usize> = FEATURES.iter().map(|c| data::column(&headers, c)).collect();
    let species = data::column(&headers, "species");

    let mut classes: Vec<String> = Vec::new();
    let mut features: Vec<Vec<f64>> = Vec::new();
    let mut target: Vec<f64> = Vec::new();
    for row in &rows {
        let mut values = Vec::with_capacity(feature_idx.len());
        let mut ok = true;
        for &i in &feature_idx {
            match row[i].parse::<f64>() {
                Ok(v) => values.push(v),
                Err(_) => {
                    ok = false;
                    break;
                }
            }
        }
        if !ok {
            continue;
        }
        let name = &row[species];
        let label = classes
            .iter()
            .position(|c| c == name)
            .unwrap_or_else(|| {
                classes.push(name.clone());
                classes.len() - 1
            });
        features.push(values);
        target.push(label as f64);
    }

    // Iris is grouped by species, so hold out every 5th row to keep the split
    // class-balanced instead of taking a contiguous tail.
    let cols: Vec<String> = FEATURES.iter().map(|s| s.to_string()).collect();
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
    let test = Frame::from_rows(te_x, cols)?;

    let mut model = RandomForest::new().n_trees(100);
    model.fit(&train)?;
    let predictions = model.predict(&test)?;

    let report = Report::for_task(&te_y, &predictions, Task::Classification);
    println!("Millwright · RandomForest on iris.csv");
    println!("  classes: {classes:?}");
    println!("  held-out rows: {}", te_y.len());
    for (name, value) in report.metrics() {
        println!("  {name:<10} = {value:.4}");
    }
    Ok(())
}
