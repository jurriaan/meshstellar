#![windows_subsystem = "console"]

mod dto;
mod import;
mod mqtt_processor;
mod proto;
mod template;
mod util;
mod web_interface;

use sqlx::SqlitePool;
use std::{env, process::exit};
use tokio::sync::oneshot::{self, error::RecvError, Receiver};
use tracing::{error, info};
use util::{config::get_config, connect_to_db, setup_tracing};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    setup_tracing();

    if let (Some(git_describe), Some(git_sha), Some(build_timestamp)) = (
        option_env!("VERGEN_GIT_DESCRIBE"),
        option_env!("VERGEN_GIT_SHA"),
        option_env!("VERGEN_BUILD_TIMESTAMP"),
    ) {
        info!(
            "Meshstellar {} ({} {})",
            git_describe, git_sha, build_timestamp
        );
    }

    let ref args: Vec<String> = env::args().collect();

    let choice = args.get(1).map(|a| a.clone()).unwrap_or("all".into());

    let http_addr = get_config().get_string("http_addr")?;
    let pool = connect_to_db().await?;

    match choice.as_str() {
        "all" => {
            let mqtt_channel = start_mqtt_processor(pool.clone());
            let import_channel = start_import(pool.clone());
            let web_channel = start_webserver(pool, http_addr);

            tokio::select! {
                res = mqtt_channel => handle_nested_result(res),
                res = import_channel => handle_nested_result(res),
                res = web_channel => handle_nested_result(res),
            }
        }
        "mqtt" => handle_result(start_mqtt_processor(pool).await?),
        "web" => handle_result(start_webserver(pool, http_addr).await?),
        "import" => handle_result(start_import(pool).await?),
        _ => println!("Make a valid choice (all, mqtt, web, import)"),
    }

    Ok(())
}

fn handle_nested_result(res: Result<anyhow::Result<()>, RecvError>) {
    match res {
        Err(err) => {
            error!("An internal error occurred: {:?}", err);

            if cfg!(windows) {
                util::windows::wait_before_close();
            }
            exit(2)
        }
        Ok(nested) => handle_result(nested),
    }
}

fn handle_result(res: anyhow::Result<()>) {
    if let Err(err) = res {
        error!("An error occurred: {:?}", err);

        if cfg!(windows) {
            util::windows::wait_before_close();
        }
        exit(1)
    }
}

fn start_webserver(pool: SqlitePool, http_addr: String) -> Receiver<anyhow::Result<()>> {
    let (sender, receiver) = oneshot::channel::<anyhow::Result<()>>();
    tokio::spawn(async move {
        sender
            .send(web_interface::start_server(pool, http_addr).await)
            .unwrap();
    });
    receiver
}

fn start_mqtt_processor(pool: SqlitePool) -> Receiver<anyhow::Result<()>> {
    let (sender, receiver) = oneshot::channel::<anyhow::Result<()>>();
    tokio::spawn(async move {
        sender
            .send(mqtt_processor::start_server(pool).await)
            .unwrap();
    });
    receiver
}

fn start_import(pool: SqlitePool) -> Receiver<anyhow::Result<()>> {
    let (sender, receiver) = oneshot::channel::<anyhow::Result<()>>();
    tokio::spawn(async move {
        sender.send(import::start_server(pool).await).unwrap();
    });
    receiver
}
