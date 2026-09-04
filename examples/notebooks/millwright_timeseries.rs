//# %% deps — first run compiles Millwright (a few minutes), then it is cached
// Millwright time-series forecasting as lag regression: predict next month from
// recent lags with a LinearRegression (smartcore backend). Univariate, classical.
:dep millwright = { version = "2.2.1", default-features = false, features = ["smartcore-backend"] }
use millwright::prelude::*;
// Trigger the one-time dependency build here (not mid-notebook):
let _ = Frame::from_rows(vec![vec![0.0]], vec!["x".into()])?;
println!("Millwright ready.");

//# %% data — a synthetic monthly series with trend + annual seasonality
// Deterministic (no randomness), so the forecast is reproducible run to run.
let n = 120usize;
let mut series: Vec<f64> = Vec::with_capacity(n);
for t in 0..n {
    let trend = 100.0 + 1.2 * t as f64;
    let angle = std::f64::consts::TAU * (t % 12) as f64 / 12.0;
    series.push(trend + 15.0 * angle.sin() + 8.0 * angle.cos() + 4.0 * (0.9 * t as f64).sin());
}
println!("Built {n} monthly points (trend + seasonality)");

//# %% features — a lagged design matrix
// For each month t (t >= 12) predict y[t] from lags 1, 2, 3 and 12 (the annual
// lag lets the linear model pick up seasonality). Chronological 24-month holdout.
let lags = [1usize, 2, 3, 12];
let cols: Vec<String> = lags.iter().map(|l| format!("lag_{l}")).collect();
let mut feats: Vec<Vec<f64>> = Vec::new();
let mut target: Vec<f64> = Vec::new();
for t in 12..n {
    feats.push(lags.iter().map(|&l| series[t - l]).collect());
    target.push(series[t]);
}
let split = feats.len() - 24;
let (tr_x, te_x) = (feats[..split].to_vec(), feats[split..].to_vec());
let (tr_y, te_y) = (target[..split].to_vec(), target[split..].to_vec());
println!("{} train / {} test rows", tr_x.len(), te_x.len());

//# %% train, evaluate, and forecast (model kept local to this cell)
let train = Dataset::new(Frame::from_rows(tr_x, cols.clone())?, tr_y)?;
let mut model = LinearRegression::new();
model.fit(&train)?;
let preds = model.predict(&Frame::from_rows(te_x.clone(), cols.clone())?)?;
let report = Report::for_task(&te_y, &preds, Task::Regression);
for (name, value) in report.metrics() {
    println!("{name:<6} = {value:.4}");
    println!("forge_metric:{name}={value}");
}
// Recursive 12-month-ahead forecast: feed predictions back in as new lags.
let mut history = series.clone();
let mut forecast: Vec<f64> = Vec::new();
for _ in 0..12 {
    let t = history.len();
    let row: Vec<f64> = lags.iter().map(|&l| history[t - l]).collect();
    let yhat = model.predict(&Frame::from_rows(vec![row], cols.clone())?)?[0];
    history.push(yhat);
    forecast.push(yhat);
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
