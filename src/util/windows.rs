use std::{
    io::{self, Write},
    time::Duration,
};

use tokio::process::Command;

pub async fn open_browser(http_addr: String) {
    tokio::time::sleep(Duration::from_secs(3)).await;

    let _ = Command::new("explorer.exe")
        .arg(format!("http://{}", http_addr))
        .output()
        .await;
}

pub fn wait_before_close() {
    io::stdout().flush().unwrap_or(());

    println!();
    println!("An error occured, please close the window...");

    loop {
        std::thread::park();
    }
}
