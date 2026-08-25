//# %% setup
let learning_rate = 0.05_f64;
let epochs = 8_usize;
println!("Configured {epochs} epochs at learning rate {learning_rate}");

//# %% data
let features = vec![1.0_f64, 2.0, 3.0, 4.0, 5.0];
let targets = vec![2.2_f64, 4.1, 6.0, 8.2, 10.1];
println!("Loaded {} training rows", features.len());

//# %% train
let mut weight = 0.0_f64;
for _epoch in 0..epochs {
    let gradient = features
        .iter()
        .zip(&targets)
        .map(|(x, y)| (weight * x - y) * x)
        .sum::<f64>()
        / features.len() as f64;
    weight -= learning_rate * gradient;
}
println!("Learned weight: {weight:.4}");

//# %% evaluate
let predictions = features.iter().map(|x| weight * x).collect::<Vec<_>>();
println!("Predictions: {predictions:?}");
"Notebook complete"
