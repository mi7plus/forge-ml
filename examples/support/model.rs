#[derive(Debug, Clone, Copy)]
pub struct LinearModel {
    weight: f64,
    bias: f64,
}

impl LinearModel {
    pub fn new(weight: f64, bias: f64) -> Self {
        Self { weight, bias }
    }

    pub fn predict(&self, input: f64) -> f64 {
        self.weight * input + self.bias
    }
}

pub fn mean_squared_error(predictions: &[f64], targets: &[f64]) -> f64 {
    predictions
        .iter()
        .zip(targets)
        .map(|(prediction, target)| (prediction - target).powi(2))
        .sum::<f64>()
        / predictions.len().max(1) as f64
}
