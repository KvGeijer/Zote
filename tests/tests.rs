use std::process::Command;

fn interpret(program: &str) -> String {
    let output = Command::new("cargo")
        .arg("run")
        .arg("--")
        .arg(program)
        .output()
        .expect("Could not run file!");

    assert!(output.status.success(), "Could not run program!");

    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn fibonachi() {
    let output = interpret("tests/programs/fib.zote");
    assert_eq!(
        output,
        "1\n[1, 1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 233, 377, 610, 987, 1597, 2584, 4181, 6765]\n6765\n"
    )
}

#[test]
fn caced_fibonachi() {
    let output = interpret("tests/programs/cached_fib.zote");
    assert_eq!(
        output,
        "[1, 1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 233, 377, 610, 987, 1597, 2584, 0, 0, 0, 17711, 1, 5]\n"
    )
}

#[test]
fn aoc_2022_1() {
    let output = interpret("aoc-2022/day01.zote");
    assert_eq!(output, "68923\n200044\n");
}

#[test]
fn aoc_2022_2() {
    let output = interpret("aoc-2022/day02.zote");
    assert_eq!(output, "12586\n13193\n");
}

#[test]
fn aoc_2022_3() {
    let output = interpret("aoc-2022/day03.zote");
    assert_eq!(output, "7568\n2780\n");
}
