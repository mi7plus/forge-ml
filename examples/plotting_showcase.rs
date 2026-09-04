//! Plotting showcase: emit one forge_plot: marker of every kind. Run in the IDE
//! (as a notebook) to populate the Plots pane, or via cargo to see the protocol.
//! Run with:  cargo run --example plotting_showcase

fn main() {
    // A tour of every Forge plot kind. Run cells top to bottom. On each plot try the
    // controls: "log X" / "log Y", "Hide outliers", and the interactions
    // (scroll = zoom, drag = pan, double-click = reset).
    println!("Plotting showcase — run each cell to add a plot to the Plots pane.");

    // A tidy cluster plus one extreme point. The auto-fit is squashed by the outlier
    // until you enable "Hide outliers", which drops the extreme 1% on each axis.
    let mut pts: Vec<String> = Vec::new();
    for i in 0..200 {
        let t = i as f64;
        let x = 5.0 + 2.0 * (t * 0.11).sin() + 0.03 * t;
        let y = 5.0 + 2.0 * (t * 0.17).cos();
        pts.push(format!("[{x:.3},{y:.3}]"));
    }
    pts.push("[80.0,80.0]".into()); // the outlier
    println!("forge_plot:{{\"version\":1,\"name\":\"scatter with outlier\",\"kind\":\"scatter\",\"series\":[{{\"name\":\"samples\",\"points\":[{}]}}]}}", pts.join(","));

    let mut linear: Vec<String> = Vec::new();
    let mut exponential: Vec<String> = Vec::new();
    for i in 0..60 {
        let x = i as f64;
        linear.push(format!("[{x},{}]", 2.0 * x + 5.0));
        exponential.push(format!("[{x},{:.4}]", (0.2 * x).exp()));
    }
    println!("forge_plot:{{\"version\":1,\"name\":\"linear vs exponential (try log Y)\",\"kind\":\"line\",\"series\":[{{\"name\":\"linear\",\"points\":[{}]}},{{\"name\":\"exponential\",\"points\":[{}]}}]}}", linear.join(","), exponential.join(","));

    let importances = [0.34, 0.27, 0.18, 0.12, 0.09];
    let values: Vec<String> = importances.iter().map(|v| format!("{v}")).collect();
    println!("forge_plot:{{\"version\":1,\"name\":\"feature importance\",\"kind\":\"feature_importance\",\"series\":[{{\"name\":\"importance\",\"values\":[{}]}}]}}", values.join(","));

    let mut values: Vec<String> = Vec::new();
    for i in 0..400 {
        // Deterministic pseudo-random in [0,1), squared to skew right.
        let u = ((i as f64 * 12.9898).sin() * 43758.5453).fract().abs();
        values.push(format!("{:.4}", u * u * 10.0));
    }
    println!("forge_plot:{{\"version\":1,\"name\":\"skewed histogram\",\"kind\":\"histogram\",\"series\":[{{\"name\":\"x\",\"values\":[{}]}}]}}", values.join(","));

    let mut area: Vec<String> = Vec::new();
    for i in 0..80 {
        let x = i as f64 * 0.15;
        area.push(format!("[{x:.3},{:.4}]", x.sin().abs() * (1.0 + 0.05 * x)));
    }
    println!("forge_plot:{{\"version\":1,\"name\":\"area under a curve\",\"kind\":\"area\",\"series\":[{{\"name\":\"signal\",\"points\":[{}]}}]}}", area.join(","));

    let group = |center: f64, spread: f64| -> String {
        (0..60)
            .map(|i| format!("{:.3}", center + spread * (i as f64 * 0.3).sin()))
            .collect::<Vec<_>>()
            .join(",")
    };
    println!("forge_plot:{{\"version\":1,\"name\":\"group spreads (box)\",\"kind\":\"box\",\"series\":[{{\"name\":\"A\",\"values\":[{}]}},{{\"name\":\"B\",\"values\":[{}]}},{{\"name\":\"C\",\"values\":[{}]}}]}}", group(3.0, 1.0), group(5.0, 1.6), group(7.0, 0.7));

    let n = 8usize;
    let mut rows: Vec<String> = Vec::new();
    for i in 0..n {
        let cells: Vec<String> = (0..n)
            .map(|j| {
                format!(
                    "{:.3}",
                    (-(i as f64 - j as f64).abs() / n as f64 * 3.0).exp()
                )
            })
            .collect();
        rows.push(format!("[{}]", cells.join(",")));
    }
    println!("forge_plot:{{\"version\":1,\"name\":\"correlation heatmap\",\"kind\":\"heatmap\",\"matrix\":[{}]}}", rows.join(","));

    let mut residuals: Vec<String> = Vec::new();
    for i in 0..100 {
        let x = i as f64;
        residuals.push(format!("[{x},{:.4}]", (x * 0.2).sin() * 0.5));
    }
    println!("forge_plot:{{\"version\":1,\"name\":\"residuals\",\"kind\":\"residual\",\"series\":[{{\"name\":\"resid\",\"points\":[{}]}}]}}", residuals.join(","));

    {
        let rows: Vec<String> = (0..20)
            .map(|i| format!("[{i},{},{:.4}]", i * 2, (i as f64 * 0.5).sin()))
            .collect();
        println!(
            "forge_table:showcase={{\"columns\":[\"i\",\"double\",\"sin\"],\"rows\":[{}]}}",
            rows.join(",")
        );
    }
}
