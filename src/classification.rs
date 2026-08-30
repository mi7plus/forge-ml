//! Native multiclass classification — softmax (multinomial logistic) regression
//! with full-batch gradient descent — plus evaluation metrics and a confusion
//! matrix. Pure and deterministic, so it is fully unit-tested and needs no GPU.

use crate::plot::{PlotKind, PlotSpec, PLOT_SPEC_VERSION};
use forge_protocol::TableData;

const MAX_ROWS: usize = 100_000;
const MAX_CLASSES: usize = 64;
const MAX_FEATURES: usize = 512;

/// A prepared classification dataset: numeric feature rows and integer labels.
#[derive(Debug, Clone, PartialEq)]
pub struct Dataset {
    pub features: Vec<Vec<f64>>,
    pub labels: Vec<usize>,
    pub feature_names: Vec<String>,
    pub class_names: Vec<String>,
}

/// A trained softmax classifier over standardized features.
#[derive(Debug, Clone)]
pub struct Classifier {
    pub classes: Vec<String>,
    pub feature_names: Vec<String>,
    means: Vec<f64>,
    scales: Vec<f64>,
    weights: Vec<Vec<f64>>, // [class][feature]
    bias: Vec<f64>,         // [class]
}

/// Per-class precision / recall / F1 and support count.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassMetric {
    pub precision: f64,
    pub recall: f64,
    pub f1: f64,
    pub support: usize,
}

/// Overall evaluation: confusion matrix (rows = actual, cols = predicted),
/// accuracy, per-class metrics, and macro-averaged F1.
#[derive(Debug, Clone, PartialEq)]
pub struct Metrics {
    pub confusion: Vec<Vec<usize>>,
    pub accuracy: f64,
    pub per_class: Vec<ClassMetric>,
    pub macro_f1: f64,
}

/// Read the chosen feature columns and target column out of a table, dropping
/// rows with any non-finite feature. Distinct target values become class ids in
/// first-seen order.
pub fn prepare(
    table: &TableData,
    feature_columns: &[String],
    target_column: &str,
) -> Result<Dataset, String> {
    if feature_columns.is_empty() {
        return Err("Choose at least one feature column".into());
    }
    if feature_columns.len() > MAX_FEATURES {
        return Err(format!("At most {MAX_FEATURES} feature columns are supported"));
    }
    if table.rows.len() > MAX_ROWS {
        return Err(format!("Classification accepts at most {MAX_ROWS} rows"));
    }
    let column_index = |name: &str| {
        table
            .columns
            .iter()
            .position(|c| c == name)
            .ok_or_else(|| format!("Column `{name}` was not found"))
    };
    let feature_indices: Vec<usize> = feature_columns
        .iter()
        .map(|name| column_index(name))
        .collect::<Result<_, _>>()?;
    let target_index = column_index(target_column)?;
    if feature_indices.contains(&target_index) {
        return Err("The target column cannot also be a feature".into());
    }

    let mut features = Vec::new();
    let mut labels = Vec::new();
    let mut class_names: Vec<String> = Vec::new();
    for row in &table.rows {
        let mut values = Vec::with_capacity(feature_indices.len());
        let mut ok = true;
        for &index in &feature_indices {
            match row.get(index).and_then(|cell| cell.parse::<f64>().ok()) {
                Some(value) if value.is_finite() => values.push(value),
                _ => {
                    ok = false;
                    break;
                }
            }
        }
        let Some(raw_target) = row.get(target_index) else {
            continue;
        };
        if !ok {
            continue;
        }
        let label = raw_target.trim();
        if label.is_empty() {
            continue;
        }
        let class = match class_names.iter().position(|c| c == label) {
            Some(id) => id,
            None => {
                if class_names.len() >= MAX_CLASSES {
                    return Err(format!("At most {MAX_CLASSES} classes are supported"));
                }
                class_names.push(label.to_owned());
                class_names.len() - 1
            }
        };
        features.push(values);
        labels.push(class);
    }

    if features.len() < 2 {
        return Err("Need at least two complete rows to train".into());
    }
    if class_names.len() < 2 {
        return Err("The target column must have at least two distinct classes".into());
    }
    Ok(Dataset {
        features,
        labels,
        feature_names: feature_columns.to_vec(),
        class_names,
    })
}

fn softmax(logits: &mut [f64]) {
    let max = logits.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let mut sum = 0.0;
    for value in logits.iter_mut() {
        *value = (*value - max).exp();
        sum += *value;
    }
    if sum > 0.0 {
        for value in logits.iter_mut() {
            *value /= sum;
        }
    }
}

impl Classifier {
    /// Train a softmax classifier by full-batch gradient descent on the
    /// standardized features. Features are standardized with per-column mean and
    /// standard deviation computed from the training data.
    pub fn train(data: &Dataset, epochs: usize, learning_rate: f64) -> Result<Self, String> {
        let n = data.features.len();
        let d = data.feature_names.len();
        let k = data.class_names.len();
        if n < 2 || d == 0 || k < 2 {
            return Err("Not enough data or classes to train".into());
        }
        if !(learning_rate > 0.0 && learning_rate <= 10.0) {
            return Err("Learning rate must be in (0, 10]".into());
        }
        let epochs = epochs.clamp(1, 100_000);

        // Standardization from the training data (guard zero-variance columns).
        let mut means = vec![0.0; d];
        for row in &data.features {
            for (j, &v) in row.iter().enumerate() {
                means[j] += v;
            }
        }
        for m in &mut means {
            *m /= n as f64;
        }
        let mut scales = vec![0.0; d];
        for row in &data.features {
            for (j, &v) in row.iter().enumerate() {
                scales[j] += (v - means[j]).powi(2);
            }
        }
        for s in &mut scales {
            *s = (*s / n as f64).sqrt();
            if *s < 1e-9 {
                *s = 1.0;
            }
        }
        let standardized: Vec<Vec<f64>> = data
            .features
            .iter()
            .map(|row| {
                row.iter()
                    .enumerate()
                    .map(|(j, &v)| (v - means[j]) / scales[j])
                    .collect()
            })
            .collect();

        let mut weights = vec![vec![0.0; d]; k];
        let mut bias = vec![0.0; k];
        for _ in 0..epochs {
            let mut grad_w = vec![vec![0.0; d]; k];
            let mut grad_b = vec![0.0; k];
            for (row, &label) in standardized.iter().zip(&data.labels) {
                let mut logits = vec![0.0; k];
                for c in 0..k {
                    let mut z = bias[c];
                    for j in 0..d {
                        z += weights[c][j] * row[j];
                    }
                    logits[c] = z;
                }
                softmax(&mut logits);
                for c in 0..k {
                    let error = logits[c] - if c == label { 1.0 } else { 0.0 };
                    grad_b[c] += error;
                    for j in 0..d {
                        grad_w[c][j] += error * row[j];
                    }
                }
            }
            let step = learning_rate / n as f64;
            for c in 0..k {
                bias[c] -= step * grad_b[c];
                for j in 0..d {
                    weights[c][j] -= step * grad_w[c][j];
                }
            }
        }

        Ok(Classifier {
            classes: data.class_names.clone(),
            feature_names: data.feature_names.clone(),
            means,
            scales,
            weights,
            bias,
        })
    }

    /// Predict the class id for one raw (unstandardized) feature row.
    pub fn predict(&self, raw: &[f64]) -> usize {
        let k = self.classes.len();
        let d = self.feature_names.len();
        let mut best = 0usize;
        let mut best_logit = f64::NEG_INFINITY;
        for c in 0..k {
            let mut z = self.bias[c];
            for j in 0..d.min(raw.len()) {
                z += self.weights[c][j] * ((raw[j] - self.means[j]) / self.scales[j]);
            }
            if z > best_logit {
                best_logit = z;
                best = c;
            }
        }
        best
    }

    /// Evaluate the classifier on a dataset, returning full metrics.
    pub fn evaluate(&self, data: &Dataset) -> Metrics {
        let predicted: Vec<usize> = data.features.iter().map(|row| self.predict(row)).collect();
        evaluate(&predicted, &data.labels, self.classes.len())
    }
}

/// Compute confusion matrix and metrics from predicted vs. actual class ids.
pub fn evaluate(predicted: &[usize], actual: &[usize], num_classes: usize) -> Metrics {
    let k = num_classes.max(1);
    let mut confusion = vec![vec![0usize; k]; k];
    let mut correct = 0usize;
    for (&p, &a) in predicted.iter().zip(actual) {
        if p < k && a < k {
            confusion[a][p] += 1;
            if p == a {
                correct += 1;
            }
        }
    }
    let total = predicted.len().max(1);
    let accuracy = correct as f64 / total as f64;

    let mut per_class = Vec::with_capacity(k);
    let mut f1_sum = 0.0;
    for c in 0..k {
        let tp = confusion[c][c];
        let predicted_c: usize = (0..k).map(|a| confusion[a][c]).sum();
        let actual_c: usize = confusion[c].iter().sum();
        let precision = if predicted_c > 0 {
            tp as f64 / predicted_c as f64
        } else {
            0.0
        };
        let recall = if actual_c > 0 {
            tp as f64 / actual_c as f64
        } else {
            0.0
        };
        let f1 = if precision + recall > 0.0 {
            2.0 * precision * recall / (precision + recall)
        } else {
            0.0
        };
        f1_sum += f1;
        per_class.push(ClassMetric {
            precision,
            recall,
            f1,
            support: actual_c,
        });
    }
    Metrics {
        confusion,
        accuracy,
        macro_f1: f1_sum / k as f64,
        per_class,
    }
}

/// A heatmap plot of the confusion matrix (rows = actual, cols = predicted).
pub fn confusion_plot(metrics: &Metrics, name: &str) -> PlotSpec {
    let matrix = metrics
        .confusion
        .iter()
        .map(|row| row.iter().map(|&count| count as f64).collect())
        .collect();
    PlotSpec {
        version: PLOT_SPEC_VERSION,
        name: name.to_owned(),
        kind: PlotKind::Heatmap,
        x_label: "predicted".to_owned(),
        y_label: "actual".to_owned(),
        series: Vec::new(),
        matrix,
        x_log: false,
        y_log: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> TableData {
        TableData {
            columns: vec!["x1".into(), "x2".into(), "label".into()],
            rows: vec![
                vec!["0.0".into(), "0.0".into(), "a".into()],
                vec!["0.1".into(), "0.2".into(), "a".into()],
                vec!["5.0".into(), "5.0".into(), "b".into()],
                vec!["4.8".into(), "5.2".into(), "b".into()],
                vec!["bad".into(), "1.0".into(), "a".into()], // dropped (non-numeric)
            ],
        }
    }

    #[test]
    fn prepare_reads_features_and_encodes_labels() {
        let data = prepare(&table(), &["x1".into(), "x2".into()], "label").unwrap();
        assert_eq!(data.features.len(), 4); // the "bad" row is dropped
        assert_eq!(data.class_names, vec!["a".to_owned(), "b".to_owned()]);
        assert_eq!(data.labels, vec![0, 0, 1, 1]);
    }

    #[test]
    fn trains_and_separates_two_classes() {
        let data = prepare(&table(), &["x1".into(), "x2".into()], "label").unwrap();
        let model = Classifier::train(&data, 400, 0.5).unwrap();
        let metrics = model.evaluate(&data);
        assert_eq!(metrics.accuracy, 1.0, "confusion: {:?}", metrics.confusion);
        assert!((metrics.macro_f1 - 1.0).abs() < 1e-9);
        // A clearly class-b point predicts b.
        assert_eq!(model.classes[model.predict(&[5.0, 5.0])], "b");
    }

    #[test]
    fn metrics_match_a_known_confusion() {
        // actual: [0,0,1,1,2], predicted: [0,1,1,1,2]
        let m = evaluate(&[0, 1, 1, 1, 2], &[0, 0, 1, 1, 2], 3);
        assert_eq!(m.confusion, vec![vec![1, 1, 0], vec![0, 2, 0], vec![0, 0, 1]]);
        assert!((m.accuracy - 4.0 / 5.0).abs() < 1e-9);
        // class 1: tp=2, predicted=3 -> precision 2/3; actual=2 -> recall 1.
        assert!((m.per_class[1].precision - 2.0 / 3.0).abs() < 1e-9);
        assert!((m.per_class[1].recall - 1.0).abs() < 1e-9);
    }

    #[test]
    fn confusion_plot_is_a_valid_heatmap() {
        let m = evaluate(&[0, 1], &[0, 1], 2);
        let plot = confusion_plot(&m, "Confusion");
        assert!(plot.validate().is_ok());
        assert_eq!(plot.kind, PlotKind::Heatmap);
    }
}
