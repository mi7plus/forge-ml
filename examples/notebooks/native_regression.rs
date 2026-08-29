//# %% dataset
// A small univariate dataset: y ~= 3x + 2 with a little noise.
// Mirrors the "use selected dataset" native-regression workflow, but computed
// inline so it runs in the shared Evcxr session with no dependencies.
let xs = vec![0.0_f64, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0];
let ys = vec![2.1_f64, 5.0, 7.8, 11.1, 13.9, 17.2, 19.8, 23.1];
println!("Loaded {} rows", xs.len());
println!("forge_table:training={{\"columns\":[\"x\",\"y\"],\"rows\":[[0.0,2.1],[1.0,5.0],[2.0,7.8],[3.0,11.1],[4.0,13.9],[5.0,17.2],[6.0,19.8],[7.0,23.1]]}}");

//# %% standardize
// Leakage-safe standardization uses only the training statistics, exactly like
// the native trainer. Here we fit on the whole set for a one-cell demo.
let n = xs.len() as f64;
let mean_x = xs.iter().sum::<f64>() / n;
let mean_y = ys.iter().sum::<f64>() / n;
let std_x = (xs.iter().map(|x| (x - mean_x).powi(2)).sum::<f64>() / n).sqrt();
let std_y = (ys.iter().map(|y| (y - mean_y).powi(2)).sum::<f64>() / n).sqrt();
let zx: Vec<f64> = xs.iter().map(|x| (x - mean_x) / std_x).collect();
let zy: Vec<f64> = ys.iter().map(|y| (y - mean_y) / std_y).collect();
println!("mean_x={mean_x:.3} std_x={std_x:.3} mean_y={mean_y:.3} std_y={std_y:.3}");

//# %% train
// Batch gradient descent on standardized data; report the loss curve each epoch.
let epochs = 60_usize;
let lr = 0.15_f64;
let mut w = 0.0_f64;
let mut b = 0.0_f64;
for _ in 0..epochs {
    let mut gw = 0.0_f64;
    let mut gb = 0.0_f64;
    let mut mse = 0.0_f64;
    for (&x, &y) in zx.iter().zip(&zy) {
        let pred = w * x + b;
        let err = pred - y;
        gw += err * x;
        gb += err;
        mse += err * err;
    }
    let m = zx.len() as f64;
    w -= lr * (gw / m);
    b -= lr * (gb / m);
    println!("forge_metric:loss={}", mse / m);
}
println!("standardized weight={w:.4} bias={b:.4}");

//# %% original-units equation
// Convert the standardized fit back to original units for a readable model card,
// just as the native artifact stores an original-unit equation.
let slope = w * (std_y / std_x);
let intercept = mean_y + std_y * b - slope * mean_x;
println!("Fitted model:  y = {slope:.3} * x + {intercept:.3}");
println!("forge_vector:coefficients={slope:.4},{intercept:.4}");

//# %% evaluate
// MAE / RMSE / R^2 on the original-unit predictions, plus actual-vs-predicted.
let preds: Vec<f64> = xs.iter().map(|x| slope * x + intercept).collect();
let mae = preds.iter().zip(&ys).map(|(p, y)| (p - y).abs()).sum::<f64>() / n;
let rmse = (preds.iter().zip(&ys).map(|(p, y)| (p - y).powi(2)).sum::<f64>() / n).sqrt();
let ss_res = preds.iter().zip(&ys).map(|(p, y)| (p - y).powi(2)).sum::<f64>();
let ss_tot = ys.iter().map(|y| (y - mean_y).powi(2)).sum::<f64>();
let r2 = 1.0 - ss_res / ss_tot;
println!("MAE={mae:.3}  RMSE={rmse:.3}  R2={r2:.4}");
println!(
    "forge_vector:predictions={}",
    preds.iter().map(|p| format!("{p:.2}")).collect::<Vec<_>>().join(",")
);
"native_regression complete"
