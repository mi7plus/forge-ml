//# %% baseline run
for epoch in 0..24 {
    let loss = (-0.14 * epoch as f64).exp();
    let accuracy = 1.0 - 0.80 * loss;
    println!("forge_metric:loss={loss}");
    println!("forge_metric:accuracy={accuracy}");
}
println!("forge_vector:weights=0.12,-0.31,0.68,0.22");
"Save this snapshot as baseline"

//# %% tuned run
for epoch in 0..24 {
    let loss = 0.82 * (-0.23 * epoch as f64).exp();
    let accuracy = 1.0 - 0.62 * loss;
    println!("forge_metric:loss={loss}");
    println!("forge_metric:accuracy={accuracy}");
}
println!("forge_vector:weights=0.18,-0.27,0.81,0.29");
"Save this snapshot as tuned"
