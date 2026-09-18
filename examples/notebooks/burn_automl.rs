//# %% deps — first run compiles Burn (several minutes), then it is cached
// AutoML in a SCRIPT: search Burn hyperparameters from a notebook cell.
//
// Forge's *built-in* AutoML (the ML-Lab button) is compiled into the IDE and
// driven from the UI, so a `//# %%` cell — which runs in the separate Evcxr
// runtime and can only see crates you bring in with `:dep` — cannot call it.
// But the engine behind it, `automl-core`, is a normal crate on crates.io, so
// you can run the same kind of search here by pairing it with `:dep burn`.
//
// Note: `automl-core` is fetched from crates.io on first build, so this cell
// needs network the first time (Burn itself comes from the bundled offline
// runtime). We model the Seaborn "tips" dataset:  tip ~ total_bill.
:dep burn = { version = "0.22.0-pre.3", default-features = false, features = ["std", "train", "flex"] }
:dep automl-core = "1.4.0"
use burn::nn::LinearConfig;
use burn::optim::{GradientsParams, SgdConfig};
use burn::tensor::{Device, Tensor};
use automl_core::prelude::*;
// Trigger the one-time Burn build here (not mid-search):
let _ = Tensor::<1>::from_floats(&[0.0f32][..], &Device::flex());
println!("Burn + automl-core ready.");

//# %% data — 50 real (total_bill, tip) rows, standardized for stable training
let tips: &[(f32, f32)] = &[
    (16.99, 1.01), (10.34, 1.66), (21.01, 3.50), (23.68, 3.31), (24.59, 3.61),
    (25.29, 4.71), (8.77, 2.00), (26.88, 3.12), (15.04, 1.96), (14.78, 3.23),
    (10.27, 1.71), (35.26, 5.00), (15.42, 1.57), (18.43, 3.00), (14.83, 3.02),
    (21.58, 3.92), (10.33, 1.67), (16.29, 3.71), (16.97, 3.50), (20.65, 3.35),
    (17.92, 4.08), (20.29, 2.75), (15.77, 2.23), (39.42, 7.58), (19.82, 3.18),
    (17.81, 2.34), (13.37, 2.00), (12.69, 2.00), (21.70, 4.30), (19.65, 3.00),
    (9.55, 1.45), (18.35, 2.50), (15.06, 3.00), (20.69, 2.45), (17.78, 3.27),
    (24.06, 3.60), (16.31, 2.00), (16.93, 3.07), (18.69, 2.31), (31.27, 5.00),
    (16.04, 2.24), (17.46, 2.54), (13.94, 3.06), (9.68, 1.32), (30.40, 5.60),
    (18.29, 3.00), (22.23, 5.00), (32.40, 6.00), (28.55, 2.05), (18.04, 3.00),
];
let xs: Vec<f32> = tips.iter().map(|(bill, _)| *bill).collect();
let ys: Vec<f32> = tips.iter().map(|(_, tip)| *tip).collect();
let n = xs.len();
let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
let std = |v: &[f32], m: f32| {
    (v.iter().map(|x| (x - m).powi(2)).sum::<f32>() / v.len() as f32)
        .sqrt()
        .max(1e-6)
};
let (mx, my) = (mean(&xs), mean(&ys));
let (sx, sy) = (std(&xs, mx), std(&ys, my));
let x_std: Vec<f32> = xs.iter().map(|v| (v - mx) / sx).collect();
let y_std: Vec<f32> = ys.iter().map(|v| (v - my) / sy).collect();
println!("Loaded {n} rows (predict tip from total_bill)");

//# %% search — automl-core drives Burn: tune learning rate + epochs
// The search space and a seeded study (identical primitives to Forge's built-in
// AutoML). Each trial trains the Burn linear regressor and reports its MSE.
let space = SearchSpace::new()
    .add("lr", Distribution::log_float(1e-3, 3e-1))
    .add("epochs", Distribution::int(50, 400));
let mut study = Study::builder(space)
    .minimize("mse")
    .seed(42)
    .build()
    .expect("build study");

study
    .optimize_n(
        &|p: &ParamSet, _sink: &mut dyn ReportSink| {
            let lr = p.float("lr")?;
            let epochs = p.int("epochs")?.max(1) as usize;
            // Train one Burn model with these hyperparameters (CPU Flex backend).
            let device = Device::flex().autodiff();
            device.seed(7);
            let input = Tensor::<1>::from_floats(&x_std[..], &device).reshape([n, 1]);
            let target = Tensor::<1>::from_floats(&y_std[..], &device).reshape([n, 1]);
            let mut model = LinearConfig::new(1, 1).init(&device);
            let mut optimizer = SgdConfig::new().init();
            let mut mse = 0.0f32;
            for _ in 0..epochs {
                let output = model.forward(input.clone());
                let loss = (output - target.clone()).powf_scalar(2.0).mean();
                mse = loss.clone().into_scalar::<f32>() * sy.powi(2);
                let grads = GradientsParams::from_grads(loss.backward(), &model);
                model = optimizer.step(lr, model, grads);
            }
            println!("  trial: lr {lr:.4}, {epochs:>3} epochs -> mse {mse:.4}");
            Ok(NamedMetrics::single("mse", mse as f64))
        },
        12,
    )
    .expect("search failed");

let best = study.best_trial().expect("best trial").expect("no trials");
let best_lr = best.params.float("lr").expect("lr");
let best_epochs = best.params.int("epochs").expect("epochs");
let best_mse = best.final_value("mse").expect("mse");
println!("\nBest: lr {best_lr:.4}, {best_epochs} epochs -> mse {best_mse:.4} (rmse ${:.3})", best_mse.sqrt());

//# %% explore — send tips to the Data viewer (and the built-in UI alternative)
let rows: Vec<String> = xs.iter().zip(&ys).map(|(x, y)| format!("[{x},{y}]")).collect();
println!(
    "forge_table:tips={{\"columns\":[\"total_bill\",\"tip\"],\"rows\":[{}]}}",
    rows.join(",")
);
// Prefer no dependency build? The same search is a built-in feature: select the
// `tips` table in the Data viewer, open Deep learning, set feature=total_bill /
// target=tip, and click "Run AutoML search" in the AutoML section — it runs the
// identical automl-core study over Forge's embedded Burn trainer, on your GPU.
"Tuned Burn from a script. The Deep-learning inspector does the same in one click."
