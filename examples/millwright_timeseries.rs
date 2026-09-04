//! Millwright time series: univariate forecasting as lag regression — predict the
//! next month from lags 1/2/3/12 with a LinearRegression, report a held-out
//! metric, and forecast 12 months ahead recursively. Synthetic monthly series
//! (trend + annual seasonality), deterministic and reproducible.
//!
//! Run with:  cargo run --example millwright_timeseries

use millwright::prelude::*;

fn main() -> Result<()> {
    let n = 120usize;
    let series: Vec<f64> = (0..n)
        .map(|t| {
            let trend = 100.0 + 1.2 * t as f64;
            let angle = std::f64::consts::TAU * (t % 12) as f64 / 12.0;
            trend + 15.0 * angle.sin() + 8.0 * angle.cos() + 4.0 * (0.9 * t as f64).sin()
        })
        .collect();

    // Lagged design matrix; the annual lag (12) lets the linear model pick up
    // seasonality. Chronological 24-month holdout.
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

    let train = Dataset::new(Frame::from_rows(tr_x, cols.clone())?, tr_y)?;
    let mut model = LinearRegression::new();
    model.fit(&train)?;
    let preds = model.predict(&Frame::from_rows(te_x, cols.clone())?)?;
    let report = Report::for_task(&te_y, &preds, Task::Regression);

    println!("Millwright · lag regression forecast on a synthetic monthly series");
    for (name, value) in report.metrics() {
        println!("  {name:<6} = {value:.4}");
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
    println!(
        "  next 12 months: {:?}",
        forecast.iter().map(|v| v.round()).collect::<Vec<_>>()
    );
    Ok(())
}
