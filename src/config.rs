use config::ConfigError;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, env, path::PathBuf};
use glob::glob;
use std::fs::File;
use crate::utils::interpolate::interpolate_config;
use anyhow::{Context, Result};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum HttpMethod {
    GET, POST, PUT, DELETE, // Add more as needed
}

#[derive(Debug, Deserialize, Clone)]
pub struct LoadTestConfig {
    pub initial_load: Option<usize>,
    pub max_load: Option<usize>,
    pub spawn_rate: Option<usize>,
    pub retry_count: Option<usize>,
    pub max_duration_secs: Option<usize>,
}

impl Default for LoadTestConfig {
    fn default() -> Self {
        LoadTestConfig {
            initial_load: Some(1),
            max_load: Some(10),
            spawn_rate: Some(1),
            retry_count: Some(0),
            max_duration_secs: Some(60),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct ApiConfig {
    pub name: String,
    pub task_order: Option<usize>,
    pub url: String,
    pub headers: HashMap<String, String>,
    pub expected_field: String,
    pub response_time_threshold: u64,
    pub method: HttpMethod,
    pub body: Option<String>,
    pub body_file: Option<String>,
    pub load_test: Option<bool>,
    pub load_test_config: Option<LoadTestConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Workflow {
    pub name: String, // Add this to identify each workflow
    pub apis: Vec<ApiConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Settings {
    pub monitoring_interval_seconds: u64,
    pub log_level: String,
    pub http_timeout_seconds: u64,
    pub http_proxy_url: Option<String>,
    pub http_default_headers: HashMap<String, String>,
}

impl Settings {
    pub fn init_logging(&self) {
        env::set_var("RUST_LOG", &self.log_level);
        env_logger::init();
    }
}


pub async fn load_workflow(config_file: Option<String>, config_dir: Option<String>) -> Result<Vec<Workflow>, Box<dyn std::error::Error>> {
    let mut workflows = Vec::new();

    let config_paths = if let Some(file_path) = config_file {
        vec![PathBuf::from(file_path)]
    } else {
        let config_directory = config_dir.unwrap_or_else(|| env::var("CONFIG_DIR").unwrap_or_else(|_| "./config".to_string()));
        glob(&format!("{}/*.yml", config_directory))
            .map_err(|e| anyhow::anyhow!("Failed to read glob pattern: {}", e))?
            .filter_map(Result::ok)
            .collect::<Vec<PathBuf>>()
    };

    // Process each configuration file...
    for config_path in config_paths {
        let file = File::open(&config_path).with_context(|| format!("Failed to open config file at {:?}", config_path))?;
        let mut workflow: Workflow = serde_yaml::from_reader(file).with_context(|| format!("Failed to parse YAML from {:?}", config_path))?;
        // Assuming these functions are async and return a Result type
        interpolate_config(&mut workflow); // Adjust this if necessary
        validate_settings(&mut workflow)?; // Ensure this is compatible with async context

        workflows.push(workflow);
    }

    Ok(workflows)
}

fn validate_settings(workflow: &mut Workflow) -> Result<(), ConfigError> {
    for api in workflow.apis.iter_mut() {
        if api.url.is_empty() {
            return Err(ConfigError::Message(format!("API URL is missing in the configuration for '{}'.", api.name)));
        }
        if api.load_test.unwrap_or(false) && api.load_test_config.is_none() {
            log::warn!("Missing load_test_config for '{}'. Using default values.", api.name);
            api.load_test_config = Some(LoadTestConfig::default());
        }
    }
    Ok(())
}

