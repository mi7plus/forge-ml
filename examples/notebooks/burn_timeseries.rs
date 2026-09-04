//# %% deps — first run compiles Burn (several minutes), then it is cached
// Burn time-series forecasting: a linear autoregressive model over a sliding
// window of the last k months, trained with SGD on the Flex CPU backend.
:dep burn = { version = "0.22.0-pre.3", default-features = false, features = ["std", "train", "flex"] }
use burn::nn::LinearConfig;
use burn::optim::{GradientsParams, SgdConfig};
use burn::tensor::{Device, Tensor};
// Trigger the one-time dependency build here (not mid-notebook):
let _ = Tensor::<1>::from_floats(&[0.0f32][..], &Device::flex());
println!("Burn ready.");

//# %% data — a synthetic monthly series (trend + annual seasonality)
let n = 120usize;
let mut series: Vec<f32> = Vec::with_capacity(n);
for t in 0..n {
    let trend = 100.0 + 1.2 * t as f32;
    let angle = std::f32::consts::TAU * (t % 12) as f32 / 12.0;
    series.push(trend + 15.0 * angle.sin() + 8.0 * angle.cos() + 4.0 * (0.9 * t as f32).sin());
}
println!("Built {n} monthly points");

//# %% train — a linear AR(k) forecaster over a sliding window
let k = 12usize; // predict the next month from the previous 12
// Standardize the whole series for stable SGD.
let mean = series.iter().sum::<f32>() / n as f32;
let scale = (series.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / n as f32)
    .sqrt()
    .max(1e-6);
let z: Vec<f32> = series.iter().map(|v| (v - mean) / scale).collect();
// Sliding windows: each row is k lags, the target is the next value.
let mut flat_x: Vec<f32> = Vec::new();
let mut flat_y: Vec<f32> = Vec::new();
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
let mut mse = 0.0f32;
for epoch in 1..=600 {
    let output = model.forward(input.clone());
    let loss = (output - target.clone()).powf_scalar(2.0).mean();
    mse = loss.clone().into_scalar::<f32>() * scale.powi(2);
    if epoch % 150 == 0 || epoch == 1 {
        println!("epoch {epoch:>3}  rmse = {:.3}", mse.sqrt());
    }
    let grads = GradientsParams::from_grads(loss.backward(), &model);
    model = optimizer.step(0.1, model, grads);
}
println!("forge_metric:rmse={}", mse.sqrt());

// Recursive 12-step forecast, in the same cell to avoid persisting Burn types
// across notebook cells. Predictions feed back in as the newest lag.
let mut hist: Vec<f32> = z.clone();
let mut forecast: Vec<f32> = Vec::new();
for _ in 0..12 {
    let t = hist.len();
    let window: Vec<f32> = (0..k).map(|j| hist[t - k + j]).collect();
    let x = Tensor::<1>::from_floats(&window[..], &device).reshape([1, k]);
    let yhat_z = model.forward(x).into_scalar::<f32>();
    hist.push(yhat_z);
    forecast.push(yhat_z * scale + mean);
}
println!("next 12 months: {:?}", forecast.iter().map(|v| v.round()).collect::<Vec<_>>());

//# %% explore — series + forecast to Plots, the table to the Data viewer
{
    let rows: Vec<String> = (0..n).map(|t| format!("[{},{}]", t, series[t])).collect();
    println!("forge_table:timeseries={{\"columns\":[\"month\",\"value\"],\"rows\":[{}]}}", rows.join(","));
}
{
    let actual: Vec<String> = (0..n).map(|t| format!("[{},{}]", t, series[t])).collect();
    let fc: Vec<String> = forecast.iter().enumerate().map(|(i, v)| format!("[{},{}]", n + i, v)).collect();
    println!("forge_plot:{{\"version\":1,\"name\":\"monthly series + 12-month forecast\",\"kind\":\"line\",\"series\":[{{\"name\":\"actual\",\"points\":[{}]}},{{\"name\":\"forecast\",\"points\":[{}]}}]}}", actual.join(","), fc.join(","));
}
