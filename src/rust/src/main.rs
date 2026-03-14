use assyplan::run;

fn main() {
    if let Err(e) = run() {
        eprintln!("Error running app: {}", e);
    }
}
