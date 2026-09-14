//# %% generate a synthetic dataset
// A toy regression: y ≈ 2·x + 1 with a little noise.
let n = 40;
let data: Vec<(f64, f64)> = (0..n)
    .map(|i| {
        let x = i as f64 / n as f64 * 4.0;
        let noise = ((i * 7 % 11) as f64 - 5.0) * 0.06;
        (x, 2.0 * x + 1.0 + noise)
    })
    .collect();
format!("{} points generated", data.len())

//# %% send it to the Data viewer
let rows = data
    .iter()
    .map(|(x, y)| format!("[{x:.3},{y:.3}]"))
    .collect::<Vec<_>>()
    .join(",");
println!(r#"forge_table:samples={{"columns":["x","y"],"rows":[{}]}}"#, rows);

//# %% plot the raw points
let points = data
    .iter()
    .map(|(x, y)| format!("[{x:.3},{y:.3}]"))
    .collect::<Vec<_>>()
    .join(",");
let series = format!(r#"{{"name":"data","points":[{}]}}"#, points);
println!(r#"forge_plot:{{"version":1,"name":"samples","kind":"scatter","series":[{}]}}"#, series);

//# %% fit y = w·x + b with gradient descent
let (mut w, mut b) = (0.0_f64, 0.0_f64);
let lr = 0.08;
for _step in 0..150 {
    let (mut gw, mut gb) = (0.0, 0.0);
    for (x, y) in &data {
        let err = (w * x + b) - y;
        gw += err * x;
        gb += err;
    }
    let m = data.len() as f64;
    w -= lr * gw / m;
    b -= lr * gb / m;
    let loss = data.iter().map(|(x, y)| (w * x + b - y).powi(2)).sum::<f64>() / m;
    println!("forge_metric:loss={loss}");
}
format!("learned  y = {w:.3}·x + {b:.3}")

//# %% overlay the fitted line
let data_pts = data
    .iter()
    .map(|(x, y)| format!("[{x:.3},{y:.3}]"))
    .collect::<Vec<_>>()
    .join(",");
let fit_pts = (0..=8)
    .map(|k| {
        let x = k as f64 * 0.5;
        format!("[{x:.3},{:.3}]", w * x + b)
    })
    .collect::<Vec<_>>()
    .join(",");
let data_series = format!(r#"{{"name":"data","points":[{}]}}"#, data_pts);
let fit_series = format!(r#"{{"name":"fit","points":[{}]}}"#, fit_pts);
println!(
    r#"forge_plot:{{"version":1,"name":"data + fit","kind":"line","series":[{},{}]}}"#,
    data_series, fit_series
);
//# %% summarize the fit as a saveable run
let m = data.len() as f64;
let mean_y = data.iter().map(|(_, y)| *y).sum::<f64>() / m;
let ss_res: f64 = data.iter().map(|(x, y)| (w * x + b - y).powi(2)).sum();
let ss_tot: f64 = data.iter().map(|(_, y)| (y - mean_y).powi(2)).sum();
let final_loss = ss_res / m;
let r_squared = 1.0 - ss_res / ss_tot;
println!("forge_metric:final_loss={final_loss}");
println!("forge_metric:r_squared={r_squared}");
format!("final loss {final_loss:.4}   R² {r_squared:.4}")
