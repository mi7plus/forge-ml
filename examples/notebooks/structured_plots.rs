//# %% line plot
// Structured `forge_plot:` JSON drives the Plots pane. Each cell below emits one
// plot family so you can exercise history, reordering, series toggles, and the
// SVG/PNG/PDF/interactive-HTML exports. The payload after `forge_plot:` is a
// single JSON object on one line.
println!(r#"forge_plot:{"name":"loss-curve","kind":"line","series":[{"label":"train","points":[[0,1.0],[1,0.62],[2,0.41],[3,0.28],[4,0.2],[5,0.15]]},{"label":"val","points":[[0,1.05],[1,0.7],[2,0.5],[3,0.4],[4,0.36],[5,0.34]]}]}"#);

//# %% scatter plot
println!(r#"forge_plot:{"name":"actual-vs-predicted","kind":"scatter","series":[{"label":"points","points":[[2.1,2.3],[5.0,4.8],[7.8,7.9],[11.1,10.9],[13.9,14.2],[17.2,17.0]]}]}"#);

//# %% bar plot
println!(r#"forge_plot:{"name":"class-counts","kind":"bars","series":[{"label":"count","points":[[0,124],[1,98],[2,143],[3,77]]}]}"#);

//# %% histogram
println!(r#"forge_plot:{"name":"residuals","kind":"histogram","series":[{"label":"error","values":[-0.4,-0.2,-0.1,0.0,0.0,0.1,0.1,0.2,-0.3,0.05,0.15,-0.05,0.25,-0.15,0.0]}]}"#);

//# %% legacy metric + vector
// The classic `forge_metric:` / `forge_vector:` markers still work alongside the
// structured plots: metrics become live line charts, vectors become bar plots.
for epoch in 0..12 {
    let loss = (-0.3 * epoch as f64).exp();
    println!("forge_metric:accuracy={}", 1.0 - loss * 0.6);
}
println!("forge_vector:feature_importance=0.42,0.31,0.18,0.06,0.03");
"structured_plots complete"
