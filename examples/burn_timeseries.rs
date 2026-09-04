//! Burn time series: a linear autoregressive forecaster over a sliding window of
//! the last 12 months, SGD-trained on the Flex backend, on a synthetic monthly
//! series (trend + annual seasonality). Deterministic, so it is reproducible.
//!
//! Run with:  cargo run --example burn_timeseries

use burn::nn::LinearConfig;
use burn::optim::{GradientsParams, SgdConfig};
use burn::tensor::{Device, Tensor};

fn main() {
    let n = 120usize;
    let series: Vec<f32> = (0..n)
        .map(|t| {
            let trend = 100.0 + 1.2 * t as f32;
            let angle = std::f32::consts::TAU * (t % 12) as f32 / 12.0;
            trend + 15.0 * angle.sin() + 8.0 * angle.cos() + 4.0 * (0.9 * t as f32).sin()
        })
        .collect();

    let k = 12usize; // predict the next month from the previous 12
    let mean = series.iter().sum::<f32>() / n as f32;
    let scale = (series.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / n as f32)
        .sqrt()
        .max(1e-6);
    let z: Vec<f32> = series.iter().map(|v| (v - mean) / scale).collect();

    let mut flat_x = Vec::new();
    let mut flat_y = Vec::new();
    for t in k..n {
        for j in 0..k {
            flat_x.push(z[t - k + j]);
        }
        flat_y.push(z[t]);
    }
    let rows = n - k;

    let device = Device::flex().autodiff();
    device.seed(7);
    let input = Tensor::<1>::from_floats(&flat_x[..], &device).reshape([rows, k]);
    let target = Tensor::<1>::from_floats(&flat_y[..], &device).reshape([rows, 1]);
    let mut model = LinearConfig::new(k, 1).init(&device);
    let mut optimizer = SgdConfig::new().init();

    println!("Burn · AR({k}) forecaster on a synthetic monthly series");
    let mut mse = 0.0f32;
    for epoch in 1..=600 {
        let output = model.forward(input.clone());
        let loss = (output - target.clone()).powf_scalar(2.0).mean();
        mse = loss.clone().into_scalar::<f32>() * scale.powi(2);
        if epoch % 150 == 0 || epoch == 1 {
            println!("  epoch {epoch:>3}  rmse = {:.3}", mse.sqrt());
        }
        let grads = GradientsParams::from_grads(loss.backward(), &model);
        model = optimizer.step(0.1, model, grads);
    }
    println!("  final rmse = {:.3}", mse.sqrt());

    // Recursive 12-step forecast: feed predictions back in as the newest lag.
    let mut hist = z.clone();
    let mut forecast = Vec::new();
    for _ in 0..12 {
        let t = hist.len();
        let window: Vec<f32> = (0..k).map(|j| hist[t - k + j]).collect();
        let x = Tensor::<1>::from_floats(&window[..], &device).reshape([1, k]);
        let yhat_z = model.forward(x).into_scalar::<f32>();
        hist.push(yhat_z);
        forecast.push(yhat_z * scale + mean);
    }
    println!(
        "  next 12 months: {:?}",
        forecast.iter().map(|v| v.round()).collect::<Vec<_>>()
    );
}
