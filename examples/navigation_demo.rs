#[path = "support/model.rs"]
mod model;

use model::{mean_squared_error, LinearModel};

// forge: expose-main
fn main() {
    let model: LinearModel = LinearModel::new(1.9, 0.2);
    let inputs: [f64; 4] = [1.0, 2.0, 3.0, 4.0];
    let targets: [f64; 4] = [2.0, 4.1, 5.9, 8.2];
    let predictions: [f64; 4] = inputs.map(|value| model.predict(value));
    let loss: f64 = mean_squared_error(&predictions, &targets);
    println!("predictions={predictions:?}, loss={loss:.4}");
    println!(
        "forge_vector:inputs={}",
        inputs.map(|value| value.to_string()).join(",")
    );
    println!(
        "forge_vector:targets={}",
        targets.map(|value| value.to_string()).join(",")
    );
    println!(
        "forge_vector:predictions={}",
        predictions.map(|value| value.to_string()).join(",")
    );
    println!("forge_metric:loss={loss}");
}
