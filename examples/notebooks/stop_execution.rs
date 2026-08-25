//# %% long running job
println!("Long-running job started. Use the Stop button before it finishes.");
for step in 1..=60 {
    std::thread::sleep(std::time::Duration::from_millis(500));
    if step % 5 == 0 {
        println!("Completed step {step}/60");
    }
}
"Long-running job completed normally"

//# %% verify restarted runtime
println!("If this runs after stopping the previous cell, the clean runtime is ready.");
