//! Burn regression: fit a single-layer linear model (tip ~ total_bill) with SGD
//! on the embedded Flex backend, standardizing the columns for stable training.
//!
//! Run with:  cargo run --example burn_regression

#[path = "support/data.rs"]
#[allow(dead_code)]
mod data;

use burn::nn::LinearConfig;
use burn::optim::{GradientsParams, SgdConfig};
use burn::tensor::{Device, Tensor};

/// Column mean and (population) standard deviation, used to standardize inputs.
struct Scale {
    mean: f32,
    std: f32,
}

impl Scale {
    fn fit(values: &[f32]) -> Self {
        let n = values.len() as f32;
        let mean = values.iter().sum::<f32>() / n;
        let var = values.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / n;
        Scale {
            mean,
            std: var.sqrt().max(1e-6),
        }
    }
    fn apply(&self, v: f32) -> f32 {
        (v - self.mean) / self.std
    }
}

fn main() {
    let (headers, rows) = data::read_csv(data::dataset_path("tips.csv"));
    let bill = data::column(&headers, "total_bill");
    let tip = data::column(&headers, "tip");

    let mut xs = Vec::new();
    let mut ys = Vec::new();
    for row in &rows {
        if let (Ok(b), Ok(t)) = (row[bill].parse::<f32>(), row[tip].parse::<f32>()) {
            xs.push(b);
            ys.push(t);
        }
    }
    let x_scale = Scale::fit(&xs);
    let y_scale = Scale::fit(&ys);
    let x_std: Vec<f32> = xs.iter().map(|&v| x_scale.apply(v)).collect();
    let y_std: Vec<f32> = ys.iter().map(|&v| y_scale.apply(v)).collect();
    let n = xs.len();

    let device = Device::flex().autodiff();
    device.seed(7);
    let input = Tensor::<1>::from_floats(&x_std[..], &device).reshape([n, 1]);
    let target = Tensor::<1>::from_floats(&y_std[..], &device).reshape([n, 1]);

    let mut model = LinearConfig::new(1, 1).init(&device);
    let mut optimizer = SgdConfig::new().init();
    let learning_rate = 0.05;

    println!("Burn · linear regression on tips.csv (predict tip from total_bill)");
    let mut final_loss = 0.0;
    for epoch in 1..=400 {
        let output = model.forward(input.clone());
        let loss = (output - target.clone()).powf_scalar(2.0).mean();
        // Report loss back in the original (dollar²) scale.
        final_loss = loss.clone().into_scalar::<f32>() * y_scale.std.powi(2);
        if epoch % 100 == 0 || epoch == 1 {
            println!("  epoch {epoch:>3}  mse = {final_loss:.4}");
        }
        let grads = loss.backward();
        let grads = GradientsParams::from_grads(grads, &model);
        model = optimizer.step(learning_rate, model, grads);
    }
    println!("  final rmse = ${:.4}", final_loss.sqrt());

    // Probe: predict the tip on a $25 bill, un-standardizing the output.
    let probe = Tensor::<1>::from_floats(&[x_scale.apply(25.0)][..], &device).reshape([1, 1]);
    let pred_std = model.forward(probe).into_scalar::<f32>();
    let pred = pred_std * y_scale.std + y_scale.mean;
    println!("  predicted tip for a $25 bill: ${pred:.2}");
}
