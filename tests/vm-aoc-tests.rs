use std::process::Command;

/// Interprets a file in 'aoc-2022/vm-solutions/{name}.zote
fn interpret_day(name: &str) -> String {
    let output = Command::new("cargo")
        .arg("run")
        .arg("--")
        .arg(format!("aoc-2022/vm-solutions/{name}.zote"))
        .output()
        .expect("Could not run file!");

    assert!(output.status.success(), "Could not run program!");
    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn aoc_2022_1_vm() {
    let output = interpret_day("day01");
    assert_eq!(output, "68923\n200044\n");
}
