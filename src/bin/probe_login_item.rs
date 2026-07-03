fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("status") => {
            let status = auto_reverse::platform::macos::login_item::status();
            println!("{:?} -> {}", status, status.summary());
        }
        Some("register") => {
            match auto_reverse::platform::macos::login_item::register() {
                Ok(()) => println!("register: ok"),
                Err(e) => println!("register: error: {e}"),
            }
            let status = auto_reverse::platform::macos::login_item::status();
            println!("post-register status: {:?} -> {}", status, status.summary());
        }
        Some("unregister") => {
            match auto_reverse::platform::macos::login_item::unregister() {
                Ok(()) => println!("unregister: ok"),
                Err(e) => println!("unregister: error: {e}"),
            }
            let status = auto_reverse::platform::macos::login_item::status();
            println!(
                "post-unregister status: {:?} -> {}",
                status,
                status.summary()
            );
        }
        _ => println!("usage: probe_login_item <status|register|unregister>"),
    }
}
