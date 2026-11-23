use console::{style, Emoji};
use self_update::cargo_crate_version;

const OWNER: &str = "dantebarbieri";
const REPO: &str = "wake";

fn err(e: impl std::fmt::Display) -> String {
    format!("self-update failed: {e}")
}

pub fn run(check_only: bool, quiet: bool) -> Result<bool, String> {
    let exe = std::env::current_exe().map_err(err)?;
    if exe.starts_with("/nix/store") {
        return Err("this wake is installed by Nix; it updates with the system flake".into());
    }
    let current = cargo_crate_version!();
    let updater = self_update::backends::github::Update::configure()
        .repo_owner(OWNER)
        .repo_name(REPO)
        .bin_name("wake")
        .current_version(current)
        .show_download_progress(!quiet && console::user_attended_stderr())
        .show_output(false)
        .no_confirm(true)
        .build()
        .map_err(err)?;
    let releases = updater.get_latest_release().map_err(err)?;
    let Some(latest) = releases.latest() else {
        return Err(err("no releases published"));
    };
    let latest = latest.version().to_string();
    let newer = self_update::version::bump_is_greater(current, &latest).map_err(err)?;
    if !newer {
        if !quiet {
            println!(
                "{} wake {current} is up to date",
                style(Emoji("✔", "+")).green()
            );
        }
        return Ok(true);
    }
    if check_only {
        println!(
            "{} wake {} is available (this is {current}); run {}",
            style(Emoji("▲", "!")).yellow(),
            style(&latest).bold(),
            style("wake self-update").cyan()
        );
        return Ok(true);
    }
    let status = updater.update().map_err(err)?;
    if !quiet {
        println!(
            "{} updated wake {current} → {}",
            style(Emoji("✔", "+")).green(),
            style(status.version()).bold()
        );
    }
    Ok(true)
}
