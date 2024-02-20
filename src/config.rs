use config::{Config, ConfigError, File, Environment};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, env, fs};
use std::path::Path;

use crate::utils::interpolate::interpolate_config;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum HttpMethod {
    GET,
    POST,
    PUT,
    DELETE,
    // Add more as needed
}


#[derive(Debug, Deserialize, Clone)]
pub struct LoadTestConfig {
    pub initial_load: Option<usize>,
    pub max_load: Option<usize>,
    pub spawn_rate: Option<usize>, // Consider using a more appropriate type if needed
    pub retry_count: Option<usize>, // Number of retries for the load test
    pub max_duration_secs: Option<usize>, // Maximum duration of the load test in seconds
}

impl Default for LoadTestConfig {
    fn default() -> Self {
        LoadTestConfig {
            initial_load: Some(1), // Default initial load
            max_load: Some(10), // Default maximum load
            spawn_rate: Some(1), // Default spawn rate
            retry_count: Some(0), // Default retry count
            max_duration_secs: Some(60), // Default maximum duration in seconds
        }
    }
}


#[derive(Debug, Deserialize, Clone)]
pub struct ApiConfig {
    pub name: String,  // User-defined name to describe the action
    pub task_order: Option<usize>,  // Order in which tasks are executed
    pub url: String,
    pub headers: HashMap<String, String>,
    pub expected_field: String,
    pub response_time_threshold: u64,
    pub method: HttpMethod, // New: Specify the HTTP method (e.g., "GET", "POST")
    pub body: Option<String>, // Direct inline body (kept for simplicity or backward compatibility)
    pub body_file: Option<String>, // Path to a file containing the request body
    // Include LoadTestConfig as an optional to support APIs without load testing
    pub load_test: Option<bool>,
    pub load_test_config: Option<LoadTestConfig>,
}


#[derive(Debug, Deserialize, Clone)]
pub struct Settings {
    pub apis: Vec<ApiConfig>,
    pub monitoring_interval_seconds: u64,
    pub log_level: String, // New: Specify log level in the configuration
    pub http_timeout_seconds: u64,
    pub http_proxy_url: Option<String>,
    pub http_default_headers: HashMap<String, String>,
}

impl Settings {
    // New: A function to initialize logging based on the configuration
    pub fn init_logging(&self) {
        std::env::set_var("RUST_LOG", &self.log_level);
        env_logger::init();
    }
}


pub async fn load_config() -> Result<Settings, ConfigError> {
    // Try to get the configuration directory from an environment variable; use a default if not found
    let config_dir = env::var("CONFIG_DIR").unwrap_or_else(|_| "./config".to_string());
    log::info!("Loading configuration from directory: {}", config_dir);

    let config_paths = ["config.yaml", "config.yml", "config.toml"]
        .iter()
        .map(|file| format!("{}/{}", config_dir, file)) // Correctly format the directory path with each file name
        .collect::<Vec<String>>();

    let mut config_builder = Config::builder();
    let mut found = false;

    for path_str in &config_paths {
        let path = Path::new(path_str);
        // Check if the file exists and is not empty before adding it as a source
        if let Ok(metadata) = fs::metadata(&path) {
            if metadata.len() > 0 {
                config_builder = config_builder.add_source(File::with_name(path.to_str().unwrap()).required(false));
                log::info!("Found configuration file: {}", path.display());
                found = true;
                break; // Stop searching after the first valid config file is found
            }
        }
    }

    if !found {
        log::error!("No configuration file found or all are empty in directory: {}", config_dir);
        return Err(ConfigError::Message("No valid configuration file found.".to_string()));
    }

    // Add environment variables as the highest precedence source
    config_builder = config_builder.add_source(Environment::with_prefix("APP").separator("__"));

    let config = config_builder.build()?;

    let mut settings = config.try_deserialize::<Settings>()?;

    // Validate the settings
    validate_settings(&mut settings)?;

    // Interpolate the settings
    interpolate_config(&mut settings);


    // Log the entire settings struct
    log::info!("Configuration loaded and validated successfully. Current settings: {:?}", settings);

    Ok(settings)
}

fn validate_settings(settings: &mut Settings) -> Result<(), ConfigError> {
    for api in settings.apis.iter_mut() {
        // Ensure URL is present
        if api.url.is_empty() {
            return Err(ConfigError::Message(format!("API URL is missing in the configuration for '{}'.", api.name)));
        }

        // If marked as a load test but lacks configuration, assign default values
        if api.load_test.unwrap_or(false) && api.load_test_config.is_none() {
            log::warn!("Missing load_test_config for '{}'. Using default values.", api.name);
            api.load_test_config = Some(LoadTestConfig::default());
        }
    }

    Ok(())
}
