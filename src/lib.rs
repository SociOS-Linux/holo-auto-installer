// TODO: https://github.com/tokio-rs/tracing/issues/843
#![allow(clippy::unit_arg)]
pub mod config;
pub mod entries;
pub mod websocket;
use anyhow::Result;
pub use websocket::{AdminWebsocket, AppWebsocket};
pub mod get_apps;
use get_apps::get_all_enabled_hosted_happs;
mod install_app;
use install_app::install_holo_hosted_happs;
mod uninstall_apps;
use uninstall_apps::uninstall_removed_happs;

/// gets all the enabled happs from HHA
/// installs new happs that were enabled or registered by its provider
/// and uninstalles old happs that were disabled or deleted by its provider
pub async fn run(core_happ: &config::Happ, config: &config::Config) -> Result<()> {
    println!("Activating holo hosted apps");
    let list_of_happs = get_all_enabled_hosted_happs(core_happ, config).await?;
    install_holo_hosted_happs(&list_of_happs, config).await?;
    uninstall_removed_happs(&list_of_happs, config).await?;
    Ok(())
}
