use std::collections::HashMap;

// src/cli.rs
use clap::{value_parser, Arg, ArgAction, ArgMatches, Command};


pub fn build_cli() -> Command {
    Command::new("Workflow Runner")
        .version("1.0")
        .author("Richard Chukwu <richinex@gmail.com>")
        .about("Runs configured workflows")
        .arg(
            Arg::new("config")
                .short('c')
                .long("config")
                .value_name("FILE")
                .help("Sets a custom config file")
                .action(ArgAction::Set)
                .num_args(1),
        )
        .arg(
            Arg::new("config-dir")
                .long("config-dir")
                .value_name("DIRECTORY")
                .help("Sets the directory to load config files from")
                .action(ArgAction::Set)
                .num_args(1),
        )
        .arg(
            Arg::new("monitoring_interval_seconds")
                .long("monitoring-interval-seconds")
                .value_name("SECONDS")
                .help("Sets the monitoring interval in seconds")
                .action(ArgAction::Set)
                .num_args(1),
        )
        .arg(
            Arg::new("log_level")
                .long("log-level")
                .value_name("LEVEL")
                .help("Sets the logging level (e.g., info, debug)")
                .action(ArgAction::Set)
                .num_args(1),
        )
        .arg(
            Arg::new("http_timeout_seconds")
                .long("http-timeout-seconds")
                .value_name("SECONDS")
                .help("Sets the HTTP timeout in seconds")
                .action(ArgAction::Set)
                .num_args(1),
        )
        .arg(Arg::new("http_proxy_url")
            .long("http-proxy-url")
            .value_name("URL")
            .help("Sets the HTTP proxy URL")
            .action(ArgAction::Set)
            .num_args(1))
        .arg(Arg::new("http_default_header")
            .long("http-default-header")
            .value_name("KEY:VALUE")
            .help("Sets a default HTTP header (can be used multiple times for multiple headers)")
            .action(ArgAction::Append)
            .num_args(1)
            .value_parser(value_parser!(String)))
}


pub fn process_http_default_headers(matches: &ArgMatches) -> Result<HashMap<String, String>, String> {
    matches.get_many::<String>("http_default_header")
        .unwrap_or_default()
        .map(|header| {
            let parts: Vec<&str> = header.splitn(2, ':').collect();
            if parts.len() == 2 {
                Ok((parts[0].trim().to_string(), parts[1].trim().to_string()))
            } else {
                Err(format!("Invalid header format: {}", header))
            }
        })
        .collect::<Result<HashMap<_, _>, _>>() // Collects into a Result<HashMap, String>, propagating the first Err encountered, if any.
}