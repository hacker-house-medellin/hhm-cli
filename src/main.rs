use anyhow::{Context, bail};
use flags2env::BundledFlags2Env;
use futures_util::StreamExt;
use tokio_tungstenite::connect_async;

mod env_map;

use env_map::{EnvMap, current_env_map, env_value, get_env_map, process_argv};

const HELP: &str = "hhm-cli 0.1.0\n\nUsage: hhm-cli <command> [options]\n\nCommands:\n  health  Check the Hacker House service\n  list    List reservations\n  get     Fetch one reservation; requires --id\n  watch   Stream reservation events\n\nOptions:\n  -h, --help       Print this help\n  -V, --version    Print the CLI version\n\nConfiguration flags are defined in .cli-flags.toml.\n";
const VERSION: &str = concat!(env!("CARGO_PKG_NAME"), " ", env!("CARGO_PKG_VERSION"), "\n");

fn informational_output<I, S>(arguments: I) -> Option<&'static str>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    arguments
        .into_iter()
        .map(|argument| argument.as_ref().to_owned())
        .find_map(|argument| match argument.as_str() {
            "-h" | "--help" => Some(HELP),
            "-V" | "--version" => Some(VERSION),
            _ => None,
        })
}

fn apply_cli_flags(argv: &[String], initial: EnvMap) -> anyhow::Result<(String, EnvMap)> {
    let parser = BundledFlags2Env::new();
    parser
        .audit_config(Some(".cli-flags.toml"))
        .map_err(|error| anyhow::anyhow!("invalid .cli-flags.toml: {error}"))?;

    let parsed = parser
        .parse_structured(argv, Some(".cli-flags.toml"))
        .map_err(|error| anyhow::anyhow!("could not parse CLI arguments: {error}"))?;

    if !parsed.unknown_options.is_empty() || !parsed.errors.is_empty() {
        bail!(
            "invalid CLI arguments: unknown={:?}, errors={:?}",
            parsed.unknown_options,
            parsed.errors
        );
    }

    let command = parsed.command.clone();
    Ok((command, get_env_map(initial, parsed.provided_flags)))
}

// Encode reservation IDs as exactly one path segment, not a URL suffix.
#[derive(Debug)]
struct InvalidReservationUrl;

impl std::fmt::Display for InvalidReservationUrl {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("invalid base URL for reservation request")
    }
}

impl std::error::Error for InvalidReservationUrl {}

fn reservation_url(base: &str, id: &str) -> Result<reqwest::Url, InvalidReservationUrl> {
    let parsed = reqwest::Url::parse(base).map_err(|_| InvalidReservationUrl)?;
    append_reservation_path(parsed, id)
}

fn append_reservation_path(
    mut url: reqwest::Url,
    id: &str,
) -> Result<reqwest::Url, InvalidReservationUrl> {
    url.path_segments_mut()
        .map_err(|_| InvalidReservationUrl)?
        .pop_if_empty()
        .extend(["v1", "reservations", id]);
    Ok(url)
}

fn env_or(env: &EnvMap, key: &str, default: &str) -> String {
    env_value(env, key)
        .map(str::to_owned)
        .unwrap_or_else(|| default.to_string())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let argv = process_argv();
    if let Some(output) = informational_output(argv.iter().skip(1)) {
        print!("{output}");
        return Ok(());
    }

    let (command, env) = apply_cli_flags(&argv, current_env_map())?;
    let base = env_or(&env, "HHM_BASE_URL", "http://127.0.0.1:8080");
    let timeout = env
        .get("HHM_TIMEOUT_SECONDS")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(20);
    let output = env_or(&env, "HHM_OUTPUT", "json");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout))
        .build()?;
    let base = base.trim_end_matches('/');

    match command.as_str() {
        "health" => {
            print_response(client.get(format!("{base}/healthz")).send().await?, &output).await
        }
        "list" => {
            print_response(
                client.get(format!("{base}/v1/reservations")).send().await?,
                &output,
            )
            .await
        }
        "get" => {
            let id = env.get("HHM_ID").context("--id is required")?;
            print_response(
                client.get(reservation_url(base, id)?).send().await?,
                &output,
            )
            .await
        }
        "watch" => watch(base).await,
        _ => bail!("choose one command: health, list, get, watch"),
    }
}

async fn print_response(response: reqwest::Response, output: &str) -> anyhow::Result<()> {
    let status = response.status();
    let text = response.text().await?;
    if !status.is_success() {
        bail!("HTTP {status}: {text}");
    }
    if output == "json" {
        let value: serde_json::Value = serde_json::from_str(&text)?;
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!("{text}");
    }
    Ok(())
}

async fn watch(base: &str) -> anyhow::Result<()> {
    let ws_base = base
        .replacen("http://", "ws://", 1)
        .replacen("https://", "wss://", 1);
    let (socket, _) = connect_async(format!("{ws_base}/v1/ws")).await?;
    let (_, mut incoming) = socket.split();
    while let Some(message) = incoming.next().await {
        println!("{}", message?.into_text()?);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reservation_url_escapes_user_supplied_id_as_one_segment() {
        assert!(matches!(
            reservation_url("https://api.example.test", "a/b?admin=true#frag"),
            Ok(url) if url.as_str() == "https://api.example.test/v1/reservations/a%2Fb%3Fadmin=true%23frag"
                && url.query().is_none()
                && url.fragment().is_none()
        ));
    }

    #[test]
    fn reservation_url_preserves_configured_base_path() {
        assert!(matches!(
            reservation_url("https://api.example.test/prefix/", "r-42"),
            Ok(url) if url.as_str() == "https://api.example.test/prefix/v1/reservations/r-42"
        ));
        assert!(reservation_url("not-a-url", "r-42").is_err());
    }

    #[test]
    fn help_and_version_are_available_without_network_or_configuration() {
        assert_eq!(informational_output(["--help"]), Some(HELP));
        assert_eq!(informational_output(["-h"]), Some(HELP));
        assert_eq!(informational_output(["--version"]), Some(VERSION));
        assert_eq!(informational_output(["health"]), None);
    }

    #[test]
    fn cli_overrides_win_without_mutating_process_environment() {
        let before = std::env::var_os("HHM_OUTPUT");
        let env = get_env_map(
            EnvMap::from([("HHM_OUTPUT".into(), "text".into())]),
            [("HHM_OUTPUT".into(), "json".into())],
        );
        assert_eq!(env.get("HHM_OUTPUT").map(String::as_str), Some("json"));
        assert_eq!(env_or(&env, "HHM_OUTPUT", "text"), "json");
        assert_eq!(std::env::var_os("HHM_OUTPUT"), before);
    }

    #[test]
    fn empty_and_whitespace_env_values_are_absent() {
        for raw in ["", " ", "\t"] {
            let env = EnvMap::from([("HHM_OUTPUT".into(), raw.into())]);
            assert_eq!(env_value(&env, "HHM_OUTPUT"), None, "raw={raw:?}");
            assert_eq!(env_or(&env, "HHM_OUTPUT", "json"), "json");
        }
    }

    #[test]
    fn apply_cli_flags_merges_cli_over_base_env_without_mutation() -> anyhow::Result<()> {
        let before = std::env::var_os("HHM_OUTPUT");
        let initial = EnvMap::from([("HHM_OUTPUT".into(), "text".into())]);
        let argv = vec![
            "hhm-cli".into(),
            "health".into(),
            "--output".into(),
            "json".into(),
        ];
        let (command, env) = apply_cli_flags(&argv, initial)?;
        assert_eq!(command, "health");
        assert_eq!(env.get("HHM_OUTPUT").map(String::as_str), Some("json"));
        assert_eq!(std::env::var_os("HHM_OUTPUT"), before);
        Ok(())
    }

    #[test]
    fn apply_cli_flags_parse_failure_does_not_mutate_process_environment() {
        let before = std::env::var_os("HHM_OUTPUT");
        let initial = EnvMap::from([("HHM_OUTPUT".into(), "text".into())]);
        let argv = vec![
            "hhm-cli".into(),
            "health".into(),
            "--this-flag-is-not-declared".into(),
        ];
        assert!(apply_cli_flags(&argv, initial).is_err());
        assert_eq!(std::env::var_os("HHM_OUTPUT"), before);
    }

    #[test]
    fn source_does_not_mutate_process_environment() {
        const SRC: &str = include_str!("main.rs");
        let production = SRC.split("#[cfg(test)]").next().unwrap_or(SRC);
        assert!(!production.contains("std::env::set_var"));
        assert!(!production.contains("env::set_var"));
    }
}
