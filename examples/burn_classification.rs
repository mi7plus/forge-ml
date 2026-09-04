//! Burn classification: a small MLP (4 → 16 → 3) trained with cross-entropy on
//! the iris dataset, using the embedded Flex backend. Reports held-out accuracy.
//!
//! Run with:  cargo run --example burn_classification

#[path = "support/data.rs"]
#[allow(dead_code)]
mod data;

use burn::module::Module;
use burn::nn::loss::CrossEntropyLossConfig;
use burn::nn::{Linear, LinearConfig};
use burn::optim::{GradientsParams, SgdConfig};
use burn::tensor::activation::relu;
use burn::tensor::{Device, Int, Tensor};

const FEATURES: [&str; 4] = ["sepal_length", "sepal_width", "petal_length", "petal_width"];

#[derive(Module, Debug)]
struct Mlp {
    hidden: Linear,
    output: Linear,
}

impl Mlp {
    fn new(device: &Device, inputs: usize, hidden: usize, classes: usize) -> Self {
        Mlp {
            hidden: LinearConfig::new(inputs, hidden).init(device),
            output: LinearConfig::new(hidden, classes).init(device),
        }
    }
    fn forward(&self, x: Tensor<2>) -> Tensor<2> {
        let x = relu(self.hidden.forward(x));
        self.output.forward(x)
    }
}

fn main() {
    let (headers, rows) = data::read_csv(data::dataset_path("iris.csv"));
    let idx: Vec<usize> = FEATURES.iter().map(|c| data::column(&headers, c)).collect();
    let species = data::column(&headers, "species");

    let mut classes: Vec<String> = Vec::new();
    let mut features: Vec<[f32; 4]> = Vec::new();
    let mut labels: Vec<i64> = Vec::new();
    for row in &rows {
        let mut values = [0.0f32; 4];
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
        features.push(values);
        labels.push(label as i64);
    }

    // Standardize each feature column over the whole set.
    let n = features.len();
    let mut mean = [0.0f32; 4];
    let mut std = [0.0f32; 4];
    for col in 0..4 {
        let m = features.iter().map(|r| r[col]).sum::<f32>() / n as f32;
        let v = features.iter().map(|r| (r[col] - m).powi(2)).sum::<f32>() / n as f32;
        mean[col] = m;
        std[col] = v.sqrt().max(1e-6);
    }

    // Interleaved 80/20 split keeps all three (grouped) species in both sets.
    let (mut tr_x, mut tr_y, mut te_x, mut te_y) = (vec![], vec![], vec![], vec![]);
    for (i, (row, &y)) in features.iter().zip(&labels).enumerate() {
        let standardized: Vec<f32> = (0..4).map(|c| (row[c] - mean[c]) / std[c]).collect();
        if i % 5 == 0 {
            te_x.extend(standardized);
            te_y.push(y);
        } else {
            tr_x.extend(standardized);
            tr_y.push(y);
        }
    }
    let n_train = tr_y.len();
    let n_test = te_y.len();

    let device = Device::flex().autodiff();
    device.seed(7);
    let train_x = Tensor::<1>::from_floats(&tr_x[..], &device).reshape([n_train, 4]);
    let train_y = Tensor::<1, Int>::from_ints(&tr_y[..], &device);
    let test_x = Tensor::<1>::from_floats(&te_x[..], &device).reshape([n_test, 4]);

    let mut model = Mlp::new(&device, 4, 16, classes.len());
    let mut optimizer = SgdConfig::new().init();
    let loss_fn = CrossEntropyLossConfig::new().init(&device);
    let learning_rate = 0.1;

    println!("Burn · MLP(4→16→{}) on iris.csv", classes.len());
    println!("  classes: {classes:?}");
    for epoch in 1..=300 {
        let logits = model.forward(train_x.clone());
        let loss = loss_fn.forward(logits, train_y.clone());
        if epoch % 100 == 0 || epoch == 1 {
            println!(
                "  epoch {epoch:>3}  loss = {:.4}",
                loss.clone().into_scalar::<f32>()
            );
        }
        let grads = loss.backward();
        let grads = GradientsParams::from_grads(grads, &model);
        model = optimizer.step(learning_rate, model, grads);
    }

    // Held-out accuracy: argmax of the logits per row.
    let logits: Vec<f32> = model
        .forward(test_x)
        .try_into_vec_as::<f32>()
        .expect("read test logits");
    let k = classes.len();
    let mut correct = 0;
    for (row, &truth) in logits.chunks(k).zip(&te_y) {
        let pred = row
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i as i64)
            .unwrap();
        if pred == truth {
            correct += 1;
        }
    }
    println!(
        "  held-out accuracy = {:.4} ({correct}/{n_test})",
        correct as f64 / n_test as f64
    );
}
