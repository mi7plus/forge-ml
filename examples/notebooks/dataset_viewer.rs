//# %% build a small labeled dataset
let samples = vec![
    (0.2_f64, 1.1_f64, "cat"),
    (0.7, 1.8, "dog"),
    (1.1, 2.4, "cat"),
    (1.8, 3.2, "dog"),
    (2.4, 4.0, "cat"),
];

//# %% publish it to Forge's Data pane
let rows = samples
    .iter()
    .map(|(x, y, label)| format!(r#"[{x},{y},"{label}"]"#))
    .collect::<Vec<_>>();
println!(
    r#"forge_table:training_samples={{"columns":["feature_x","feature_y","label"],"rows":[{}]}}"#,
    rows.join(",")
);

//# %% inspect a numeric column as a vector and plot
let feature_x = samples.iter().map(|sample| sample.0).collect::<Vec<_>>();
println!(
    "forge_vector:feature_x={}",
    feature_x
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",")
);
