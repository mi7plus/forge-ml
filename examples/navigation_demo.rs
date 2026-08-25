#[path = "support/model.rs"]
mod model;

use model::{mean_squared_error, LinearModel};

fn main() {
    let model = LinearModel::new(1.9, 0.2);
    let inputs = [1.0, 2.0, 3.0, 4.0];
    let targets = [2.0, 4.1, 5.9, 8.2];
    let predictions = inputs.map(|value| model.predict(value));
    let loss = mean_squared_error(&predictions, &targets);
    println!("predictions={predictions:?}, loss={loss:.4}");
}
