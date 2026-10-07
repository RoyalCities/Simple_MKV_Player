#[path = "../media/libmpv.rs"]
mod libmpv;

fn main() {
    println!("Simple MKV Player - libmpv smoke test");
    println!();

    match libmpv::LibMpv::smoke_test() {
        Ok(message) => {
            println!("SUCCESS");
            println!("{message}");
        }

        Err(error) => {
            eprintln!("FAILED");
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
