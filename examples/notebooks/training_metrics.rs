//# %% experiment
let run_name = "linear-regression-demo";
let epochs = 20_usize;
println!("Starting {run_name}");

//# %% dataset preview
let samples = vec![0.2_f64, 0.7, 1.1, 1.8, 2.4, 3.0];
println!("forge_vector:samples=0.2,0.7,1.1,1.8,2.4,3.0");

//# %% training loop
for epoch in 0..epochs {
    let loss = (-0.22 * epoch as f64).exp();
    let accuracy = 1.0 - loss * 0.72;
    println!("forge_metric:loss={loss}");
    println!("forge_metric:accuracy={accuracy}");
}
println!("Training finished");

//# %% model weights
let weights = vec![0.18_f64, -0.42, 0.91, 0.37, -0.12];
println!("forge_vector:weights=0.18,-0.42,0.91,0.37,-0.12");
weights
