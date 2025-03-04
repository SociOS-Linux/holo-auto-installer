pub use crate::config;
pub use crate::entries;
pub use crate::get_apps;
pub use crate::AdminWebsocket;
use anyhow::{anyhow, Context, Result};
use holochain_types::prelude::{AppManifest, MembraneProof, SerializedBytes, UnsafeBytes};
use holofuel_types::fuel::Fuel;
use mr_bundle::Bundle;
use std::{collections::HashMap, fs, path::PathBuf, str::FromStr, sync::Arc};
use tempfile::TempDir;
use tracing::{debug, info, instrument, warn};
use url::Url;

/// installs a happs that are mented to be hosted
pub async fn install_holo_hosted_happs(
    happs: &[get_apps::HappBundle],
    config: &config::Config,
) -> Result<()> {
    info!("Starting to install....");

    // Hardcoded servicelogger preferences for all the hosted happs installed
    let preferences = entries::Preferences {
        max_fuel_before_invoice: Fuel::from_str("1000")?, // MAX_TX_AMT in holofuel is currently hard-coded to 50,000
        max_time_before_invoice: vec![86400, 0],
        price_compute: Fuel::from_str("0.025")?,
        price_storage: Fuel::from_str("0.025")?,
        price_bandwidth: Fuel::from_str("0.025")?,
    }
    .save()?;

    if happs.is_empty() {
        info!("No happs registered to be enabled for hosting.");
        return Ok(());
    }

    let mut admin_websocket = AdminWebsocket::connect(config.admin_port)
        .await
        .context("failed to connect to holochain's admin interface")?;

    if let Err(error) = admin_websocket.attach_app_interface(config.happ_port).await {
        warn!(port = ?config.happ_port, ?error, "failed to start app interface, maybe it's already up?");
    }

    let active_happs = Arc::new(
        admin_websocket
            .list_running_app()
            .await
            .context("failed to get installed hApps")?,
    );

    let client = reqwest::Client::new();

    // iterate through the vec and
    // Call http://localhost/holochain-api/install_hosted_happ
    // for each WrappedActionHash to install the hosted_happ
    for get_apps::HappBundle {
        happ_id,
        bundle_url,
        is_paused,
        special_installed_app_id,
    } in happs
    {
        // if special happ is installed and do nothing if it is installed
        if special_installed_app_id.is_some()
            && active_happs.contains(&format!("{:?}::servicelogger", happ_id))
        {
            info!(
                "Special App {:?} already installed",
                special_installed_app_id
            );
            // We do not pause here because we do not want our core-app to be uninstalled ever
        }
        // Check if happ is already installed and deactivate it if happ is paused in hha
        else if active_happs.contains(&format!("{:?}", happ_id)) {
            info!("App {:?} already installed", happ_id);
            if *is_paused {
                info!("Pausing {:?}", happ_id);
                admin_websocket
                    .deactivate_app(&happ_id.0.to_string())
                    .await?;
            }
        }
        // else installed the hosted happ read-only instance
        else {
            info!("Load mem-proofs for {:?}", happ_id);
            let mem_proof: HashMap<String, MembraneProof> =
                load_mem_proof_file(bundle_url).await.unwrap_or_default();
            info!(
                "Installing happ-id {:?} with mem_proof {:?}",
                happ_id, mem_proof
            );
            let body = entries::InstallHappBody {
                happ_id: happ_id.0.to_string(),
                preferences: preferences.clone(),
                membrane_proofs: mem_proof.clone(),
            };
            let response = client
                .post("http://localhost/holochain-api/install_hosted_happ")
                .json(&body)
                .send()
                .await?;
            info!("Installed happ-id {:?}", happ_id);
            info!("Response {:?}", response);
        }
    }
    Ok(())
}

/// Temporary read-only mem-proofs solution
/// should be replaced by calling the joining-code service and getting the appropriate proof for the agent
pub async fn load_mem_proof_file(bundle_url: &str) -> Result<HashMap<String, MembraneProof>> {
    let url = Url::parse(bundle_url)?;

    let path = download_file(&url).await?;

    let bundle = Bundle::read_from_file(&path).await.unwrap();

    let AppManifest::V1(manifest) = bundle.manifest();

    Ok(manifest
        .roles
        .clone()
        .iter()
        .map(|role| {
            (
                role.name.clone(),
                Arc::new(SerializedBytes::from(UnsafeBytes::from(vec![0]))),
            ) // The read only memproof is [0] (or in base64 `AA==`)
        })
        .collect())
}

#[instrument(err, skip(url))]
pub(crate) async fn download_file(url: &Url) -> Result<PathBuf> {
    use isahc::config::RedirectPolicy;
    use isahc::prelude::*;

    let path = if url.scheme() == "file" {
        let p = PathBuf::from(url.path());
        debug!("Using: {:?}", p);
        p
    } else {
        debug!("downloading");
        let mut url = Url::clone(url);
        url.set_scheme("https")
            .map_err(|_| anyhow!("failed to set scheme to https"))?;
        let client = HttpClient::builder()
            .redirect_policy(RedirectPolicy::Follow)
            .build()
            .context("failed to initiate download request")?;
        let mut response = client
            .get(url.as_str())
            .context("failed to send GET request")?;
        if !response.status().is_success() {
            return Err(anyhow!(
                "response status code {} indicated failure",
                response.status().as_str()
            ));
        }
        let dir = TempDir::new().context("failed to create tempdir")?;
        let url_path = PathBuf::from(url.path());
        let basename = url_path
            .file_name()
            .context("failed to get basename from url")?;
        let path = dir.into_path().join(basename);
        let mut file = fs::File::create(&path).context("failed to create target file")?;
        response
            .copy_to(&mut file)
            .context("failed to write response to file")?;
        debug!("download successful");
        path
    };
    Ok(path)
}
