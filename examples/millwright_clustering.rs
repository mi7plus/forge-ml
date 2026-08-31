//! Millwright clustering: k-means (k=3) over the iris flower measurements, then
//! a cluster-vs-species cross-tab to see how well the unsupervised clusters
//! recover the real species.
//!
//! Run with:  cargo run --example millwright_clustering

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
    let mut truth: Vec<usize> = Vec::new();
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
        let label = classes.iter().position(|c| c == name).unwrap_or_else(|| {
            classes.push(name.clone());
            classes.len() - 1
        });
        features.push(values);
        truth.push(label);
    }

    let cols: Vec<String> = FEATURES.iter().map(|s| s.to_string()).collect();
    let frame = Frame::from_rows(features, cols)?;

    let k = 3;
    let mut kmeans = KMeans::new(k);
    kmeans.fit(&frame)?;
    let labels = kmeans.predict(&frame)?;

    println!("Millwright · KMeans(k={k}) on iris.csv");
    let mut sizes = vec![0usize; k];
    for &label in &labels {
        sizes[label as usize] += 1;
    }
    println!("  cluster sizes: {sizes:?}");

    // Cross-tab: rows = cluster, columns = true species.
    let mut table = vec![vec![0usize; classes.len()]; k];
    for (&label, &species) in labels.iter().zip(&truth) {
        table[label as usize][species] += 1;
    }
    print!("  cluster \\ species");
    for name in &classes {
        print!("  {name:>10}");
    }
    println!();
    for (cluster, counts) in table.iter().enumerate() {
        print!("  cluster {cluster:<8}");
        for count in counts {
            print!("  {count:>10}");
        }
        println!();
    }
    Ok(())
}
