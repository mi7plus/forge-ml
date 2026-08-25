//# %% valid setup
let values = vec![1_i32, 2, 3, 4];
println!("Values: {values:?}");

//# %% intentional type error
let total: String = values.iter().sum::<i32>();
println!("This line is unreachable after the compiler error: {total}");

//# %% intentional missing symbol
let prediction = model_that_does_not_exist.predict(&values);
prediction
