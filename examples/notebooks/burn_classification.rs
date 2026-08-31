//# %% deps — first run compiles Burn (several minutes), then it is cached
// Burn MLP (4->16->3) trained with cross-entropy on iris, as a notebook.
:dep burn = { version = "0.22.0-pre.3", default-features = false, features = ["std", "train", "flex"] }
use burn::module::Module;
use burn::nn::loss::CrossEntropyLossConfig;
use burn::nn::{Linear, LinearConfig};
use burn::optim::{GradientsParams, SgdConfig};
use burn::tensor::activation::relu;
use burn::tensor::{Device, Int, Tensor};
// Trigger the one-time dependency build here (not mid-notebook):
let _ = Tensor::<1>::from_floats(&[0.0f32][..], &Device::flex());
println!("Burn ready.");

//# %% model
#[derive(Module, Debug)]
struct Mlp { hidden: Linear, output: Linear }
impl Mlp {
    fn new(device: &Device, inputs: usize, hidden: usize, classes: usize) -> Self {
        Mlp { hidden: LinearConfig::new(inputs, hidden).init(device),
              output: LinearConfig::new(hidden, classes).init(device) }
    }
    fn forward(&self, x: Tensor<2>) -> Tensor<2> { self.output.forward(relu(self.hidden.forward(x))) }
}

//# %% data
let iris_csv = r#"sepal_length,sepal_width,petal_length,petal_width,species
5.1,3.5,1.4,0.2,setosa
4.9,3.0,1.4,0.2,setosa
4.7,3.2,1.3,0.2,setosa
4.6,3.1,1.5,0.2,setosa
5.0,3.6,1.4,0.2,setosa
5.4,3.9,1.7,0.4,setosa
4.6,3.4,1.4,0.3,setosa
5.0,3.4,1.5,0.2,setosa
4.4,2.9,1.4,0.2,setosa
4.9,3.1,1.5,0.1,setosa
5.4,3.7,1.5,0.2,setosa
4.8,3.4,1.6,0.2,setosa
4.8,3.0,1.4,0.1,setosa
4.3,3.0,1.1,0.1,setosa
5.8,4.0,1.2,0.2,setosa
5.7,4.4,1.5,0.4,setosa
5.4,3.9,1.3,0.4,setosa
5.1,3.5,1.4,0.3,setosa
5.7,3.8,1.7,0.3,setosa
5.1,3.8,1.5,0.3,setosa
5.4,3.4,1.7,0.2,setosa
5.1,3.7,1.5,0.4,setosa
4.6,3.6,1.0,0.2,setosa
5.1,3.3,1.7,0.5,setosa
4.8,3.4,1.9,0.2,setosa
5.0,3.0,1.6,0.2,setosa
5.0,3.4,1.6,0.4,setosa
5.2,3.5,1.5,0.2,setosa
5.2,3.4,1.4,0.2,setosa
4.7,3.2,1.6,0.2,setosa
4.8,3.1,1.6,0.2,setosa
5.4,3.4,1.5,0.4,setosa
5.2,4.1,1.5,0.1,setosa
5.5,4.2,1.4,0.2,setosa
4.9,3.1,1.5,0.2,setosa
5.0,3.2,1.2,0.2,setosa
5.5,3.5,1.3,0.2,setosa
4.9,3.6,1.4,0.1,setosa
4.4,3.0,1.3,0.2,setosa
5.1,3.4,1.5,0.2,setosa
5.0,3.5,1.3,0.3,setosa
4.5,2.3,1.3,0.3,setosa
4.4,3.2,1.3,0.2,setosa
5.0,3.5,1.6,0.6,setosa
5.1,3.8,1.9,0.4,setosa
4.8,3.0,1.4,0.3,setosa
5.1,3.8,1.6,0.2,setosa
4.6,3.2,1.4,0.2,setosa
5.3,3.7,1.5,0.2,setosa
5.0,3.3,1.4,0.2,setosa
7.0,3.2,4.7,1.4,versicolor
6.4,3.2,4.5,1.5,versicolor
6.9,3.1,4.9,1.5,versicolor
5.5,2.3,4.0,1.3,versicolor
6.5,2.8,4.6,1.5,versicolor
5.7,2.8,4.5,1.3,versicolor
6.3,3.3,4.7,1.6,versicolor
4.9,2.4,3.3,1.0,versicolor
6.6,2.9,4.6,1.3,versicolor
5.2,2.7,3.9,1.4,versicolor
5.0,2.0,3.5,1.0,versicolor
5.9,3.0,4.2,1.5,versicolor
6.0,2.2,4.0,1.0,versicolor
6.1,2.9,4.7,1.4,versicolor
5.6,2.9,3.6,1.3,versicolor
6.7,3.1,4.4,1.4,versicolor
5.6,3.0,4.5,1.5,versicolor
5.8,2.7,4.1,1.0,versicolor
6.2,2.2,4.5,1.5,versicolor
5.6,2.5,3.9,1.1,versicolor
5.9,3.2,4.8,1.8,versicolor
6.1,2.8,4.0,1.3,versicolor
6.3,2.5,4.9,1.5,versicolor
6.1,2.8,4.7,1.2,versicolor
6.4,2.9,4.3,1.3,versicolor
6.6,3.0,4.4,1.4,versicolor
6.8,2.8,4.8,1.4,versicolor
6.7,3.0,5.0,1.7,versicolor
6.0,2.9,4.5,1.5,versicolor
5.7,2.6,3.5,1.0,versicolor
5.5,2.4,3.8,1.1,versicolor
5.5,2.4,3.7,1.0,versicolor
5.8,2.7,3.9,1.2,versicolor
6.0,2.7,5.1,1.6,versicolor
5.4,3.0,4.5,1.5,versicolor
6.0,3.4,4.5,1.6,versicolor
6.7,3.1,4.7,1.5,versicolor
6.3,2.3,4.4,1.3,versicolor
5.6,3.0,4.1,1.3,versicolor
5.5,2.5,4.0,1.3,versicolor
5.5,2.6,4.4,1.2,versicolor
6.1,3.0,4.6,1.4,versicolor
5.8,2.6,4.0,1.2,versicolor
5.0,2.3,3.3,1.0,versicolor
5.6,2.7,4.2,1.3,versicolor
5.7,3.0,4.2,1.2,versicolor
5.7,2.9,4.2,1.3,versicolor
6.2,2.9,4.3,1.3,versicolor
5.1,2.5,3.0,1.1,versicolor
5.7,2.8,4.1,1.3,versicolor
6.3,3.3,6.0,2.5,virginica
5.8,2.7,5.1,1.9,virginica
7.1,3.0,5.9,2.1,virginica
6.3,2.9,5.6,1.8,virginica
6.5,3.0,5.8,2.2,virginica
7.6,3.0,6.6,2.1,virginica
4.9,2.5,4.5,1.7,virginica
7.3,2.9,6.3,1.8,virginica
6.7,2.5,5.8,1.8,virginica
7.2,3.6,6.1,2.5,virginica
6.5,3.2,5.1,2.0,virginica
6.4,2.7,5.3,1.9,virginica
6.8,3.0,5.5,2.1,virginica
5.7,2.5,5.0,2.0,virginica
5.8,2.8,5.1,2.4,virginica
6.4,3.2,5.3,2.3,virginica
6.5,3.0,5.5,1.8,virginica
7.7,3.8,6.7,2.2,virginica
7.7,2.6,6.9,2.3,virginica
6.0,2.2,5.0,1.5,virginica
6.9,3.2,5.7,2.3,virginica
5.6,2.8,4.9,2.0,virginica
7.7,2.8,6.7,2.0,virginica
6.3,2.7,4.9,1.8,virginica
6.7,3.3,5.7,2.1,virginica
7.2,3.2,6.0,1.8,virginica
6.2,2.8,4.8,1.8,virginica
6.1,3.0,4.9,1.8,virginica
6.4,2.8,5.6,2.1,virginica
7.2,3.0,5.8,1.6,virginica
7.4,2.8,6.1,1.9,virginica
7.9,3.8,6.4,2.0,virginica
6.4,2.8,5.6,2.2,virginica
6.3,2.8,5.1,1.5,virginica
6.1,2.6,5.6,1.4,virginica
7.7,3.0,6.1,2.3,virginica
6.3,3.4,5.6,2.4,virginica
6.4,3.1,5.5,1.8,virginica
6.0,3.0,4.8,1.8,virginica
6.9,3.1,5.4,2.1,virginica
6.7,3.1,5.6,2.4,virginica
6.9,3.1,5.1,2.3,virginica
5.8,2.7,5.1,1.9,virginica
6.8,3.2,5.9,2.3,virginica
6.7,3.3,5.7,2.5,virginica
6.7,3.0,5.2,2.3,virginica
6.3,2.5,5.0,1.9,virginica
6.5,3.0,5.2,2.0,virginica
6.2,3.4,5.4,2.3,virginica
5.9,3.0,5.1,1.8,virginica
"#;
let mut classes: Vec<String> = Vec::new();
let mut feats: Vec<[f32; 4]> = Vec::new();
let mut labels: Vec<i64> = Vec::new();
for line in iris_csv.lines().skip(1) {
    let c: Vec<&str> = line.split(',').collect();
    if c.len() < 5 { continue; }
    let mut row = [0.0f32; 4]; let mut ok = true;
    for (slot, s) in row.iter_mut().zip(&c[..4]) {
        match s.trim().parse::<f32>() { Ok(v) => *slot = v, Err(_) => { ok = false; break; } }
    }
    if !ok { continue; }
    let name = c[4].trim().to_string();
    let label = classes.iter().position(|x| *x == name).unwrap_or_else(|| { classes.push(name.clone()); classes.len() - 1 });
    feats.push(row); labels.push(label as i64);
}
// Standardize columns.
let n = feats.len();
let mut mean = [0.0f32; 4]; let mut sd = [0.0f32; 4];
for col in 0..4 {
    let m = feats.iter().map(|r| r[col]).sum::<f32>() / n as f32;
    let v = feats.iter().map(|r| (r[col] - m).powi(2)).sum::<f32>() / n as f32;
    mean[col] = m; sd[col] = v.sqrt().max(1e-6);
}
println!("Loaded {n} rows, classes: {:?}", classes);

//# %% train
let (mut tr_x, mut tr_y, mut te_x, mut te_y): (Vec<f32>, Vec<i64>, Vec<f32>, Vec<i64>) = (vec![], vec![], vec![], vec![]);
for (i, (row, &y)) in feats.iter().zip(&labels).enumerate() {
    let z: Vec<f32> = (0..4).map(|col| (row[col] - mean[col]) / sd[col]).collect();
    if i % 5 == 0 { te_x.extend(z); te_y.push(y); } else { tr_x.extend(z); tr_y.push(y); }
}
let (n_tr, n_te) = (tr_y.len(), te_y.len());
let device = Device::flex().autodiff();
device.seed(7);
let train_x = Tensor::<1>::from_floats(&tr_x[..], &device).reshape([n_tr, 4]);
let train_y = Tensor::<1, Int>::from_ints(&tr_y[..], &device);
let test_x = Tensor::<1>::from_floats(&te_x[..], &device).reshape([n_te, 4]);
let mut model = Mlp::new(&device, 4, 16, classes.len());
let mut optimizer = SgdConfig::new().init();
let loss_fn = CrossEntropyLossConfig::new().init(&device);
for epoch in 1..=300 {
    let logits = model.forward(train_x.clone());
    let loss = loss_fn.forward(logits, train_y.clone());
    if epoch % 100 == 0 || epoch == 1 { println!("epoch {epoch:>3}  loss = {:.4}", loss.clone().into_scalar::<f32>()); }
    let grads = GradientsParams::from_grads(loss.backward(), &model);
    model = optimizer.step(0.1, model, grads);
}
// Evaluate in the same cell so the model/tensor types stay local (opaque Burn
// types are awkward to persist across notebook cells).
let logits: Vec<f32> = model.forward(test_x).try_into_vec_as::<f32>().unwrap();
let kc = classes.len();
let mut correct = 0;
for (row, &truth) in logits.chunks(kc).zip(&te_y) {
    let pred = row.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).map(|(i, _)| i as i64).unwrap();
    if pred == truth { correct += 1; }
}
let acc = correct as f64 / n_te as f64;
println!("held-out accuracy = {acc:.4} ({correct}/{n_te})");
println!("forge_metric:accuracy={acc}");
