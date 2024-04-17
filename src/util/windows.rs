use std::{
    io::{self, Write},
    time::Duration,
};

use futures::Future;
use tokio::process::Command;

pub fn open_browser(http_addr: String) -> impl Future<Output = ()> {
    async move {
        tokio::time::sleep(Duration::from_secs(3)).await;

        let _ = Command::new("explorer.exe")
            .arg(format!("http://{}", http_addr))
            .output()
            .await;

        ()
    }
}

pub fn wait_before_close() {
    io::stdout().flush().unwrap_or(());

    println!();
    println!("An error occured, please close the window...");

    loop {
        std::thread::park();
    }
}
