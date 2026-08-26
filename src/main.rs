use anyhow::{Context, bail};
use flags2env::BundledFlags2Env;
use futures_util::StreamExt;
use std::collections::BTreeMap;
use tokio_tungstenite::connect_async;

type EnvMap = BTreeMap<String, String>;

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

fn merge_env(mut initial: EnvMap, overrides: impl IntoIterator<Item = (String, String)>) -> EnvMap {
    initial.extend(overrides);
    initial
}

fn apply_flags(argv: &[String], initial: EnvMap) -> anyhow::Result<(String, EnvMap)> {
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
    Ok((command, merge_env(initial, parsed.provided_flags)))
}

fn env_or(env: &EnvMap, key: &str, default: &str) -> String {
    env.get(key).cloned().unwrap_or_else(|| default.to_string())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let argv = std::env::args().collect::<Vec<_>>();
    if let Some(output) = informational_output(argv.iter().skip(1)) {
        print!("{output}");
        return Ok(());
    }

    let initial = std::env::vars().collect::<EnvMap>();
    let (command, env) = apply_flags(&argv, initial)?;
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
                client
                    .get(format!("{base}/v1/reservations/{id}"))
                    .send()
                    .await?,
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
    fn help_and_version_are_available_without_network_or_configuration() {
        assert_eq!(informational_output(["--help"]), Some(HELP));
        assert_eq!(informational_output(["-h"]), Some(HELP));
        assert_eq!(informational_output(["--version"]), Some(VERSION));
        assert_eq!(informational_output(["health"]), None);
    }

    #[test]
    fn cli_overrides_win_without_mutating_process_environment() {
        let before = std::env::var_os("HHM_OUTPUT");
        let env = merge_env(
            EnvMap::from([("HHM_OUTPUT".into(), "text".into())]),
            [("HHM_OUTPUT".into(), "json".into())],
        );
        assert_eq!(env.get("HHM_OUTPUT").map(String::as_str), Some("json"));
        assert_eq!(std::env::var_os("HHM_OUTPUT"), before);
    }
}
