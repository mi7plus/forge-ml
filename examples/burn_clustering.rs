//! Burn clustering: k-means (k=3) over the iris measurements, implemented with
//! Burn tensor ops (broadcasting distances, argmin assignment, matmul centroid
//! update) on the embedded Flex backend — no autodiff needed. Ends with a
//! cluster-vs-species cross-tab.
//!
//! Run with:  cargo run --example burn_clustering

#[path = "support/data.rs"]
#[allow(dead_code)]
mod data;

use burn::tensor::{Device, Tensor};

const FEATURES: [&str; 4] = ["sepal_length", "sepal_width", "petal_length", "petal_width"];
const K: usize = 3;
const DIMS: usize = 4;

fn main() {
    let (headers, rows) = data::read_csv(data::dataset_path("iris.csv"));
    let idx: Vec<usize> = FEATURES.iter().map(|c| data::column(&headers, c)).collect();
    let species = data::column(&headers, "species");

    let mut classes: Vec<String> = Vec::new();
    let mut flat: Vec<f32> = Vec::new();
    let mut truth: Vec<usize> = Vec::new();
    for row in &rows {
        let mut values = [0.0f32; DIMS];
        let mut ok = true;
        for (slot, &i) in values.iter_mut().zip(&idx) {
            match row[i].parse::<f32>() {
                Ok(v) => *slot = v,
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
        flat.extend(values);
        truth.push(label);
    }
    let n = truth.len();

    let device = Device::flex();
    let points = Tensor::<1>::from_floats(&flat[..], &device).reshape([n, DIMS]);

    // Seed centroids from evenly spaced rows so the three initial centres are
    // spread across the (species-grouped) dataset.
    let seed_rows = [0, n / 2, n - 1];
    let mut seeds: Vec<f32> = Vec::with_capacity(K * DIMS);
    for &r in &seed_rows {
        seeds.extend_from_slice(&flat[r * DIMS..r * DIMS + DIMS]);
    }
    let mut centroids = Tensor::<1>::from_floats(&seeds[..], &device).reshape([K, DIMS]);

    let mut assignments = vec![0usize; n];
    for _ in 0..25 {
        // Pairwise squared distances [N, K] via broadcasting [N,1,D] - [1,K,D].
        let diff = points.clone().reshape([n, 1, DIMS]) - centroids.clone().reshape([1, K, DIMS]);
        let distances = diff.powf_scalar(2.0).sum_dim(2).reshape([n, K]);
        assignments = distances
            .argmin(1)
            .reshape([n])
            .try_into_vec_as::<i64>()
            .expect("read assignments")
            .into_iter()
            .map(|v| v as usize)
            .collect();

        // Centroid update as a one-hot^T · points matmul, dividing by the count.
        let mut onehot = vec![0.0f32; n * K];
        let mut counts = [0.0f32; K];
        for (row, &cluster) in assignments.iter().enumerate() {
            onehot[row * K + cluster] = 1.0;
            counts[cluster] += 1.0;
        }
        let onehot = Tensor::<1>::from_floats(&onehot[..], &device).reshape([n, K]);
        let sums = onehot.swap_dims(0, 1).matmul(points.clone()); // [K, D]
        let divisor: Vec<f32> = counts.iter().map(|&c| c.max(1.0)).collect();
        let divisor = Tensor::<1>::from_floats(&divisor[..], &device).reshape([K, 1]);
        centroids = sums / divisor;
    }

    println!("Burn · KMeans(k={K}) on iris.csv");
    let mut sizes = [0usize; K];
    for &cluster in &assignments {
        sizes[cluster] += 1;
    }
    println!("  cluster sizes: {sizes:?}");

    // Cross-tab: rows = cluster, columns = true species.
    let mut table = vec![vec![0usize; classes.len()]; K];
    for (&cluster, &species) in assignments.iter().zip(&truth) {
        table[cluster][species] += 1;
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
}
