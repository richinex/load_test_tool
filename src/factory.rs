use log::info;

use futures::future::join_all;

use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::config::Settings;
use crate::appstate::AppState;
use crate::loadtest::LoadTest;
use crate::tasks::Task;
use crate::utils::http_client::{self, HttpClientConfig};
use std::{fs, str::FromStr};
use reqwest::{Client, RequestBuilder};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use crate::config::{ApiConfig, HttpMethod};




#[async_trait::async_trait]
pub trait ApiMonitor {
    async fn execute(&self, client: &reqwest::Client) -> Result<(), String>;
    fn describe(&self) -> String;
    fn response_time_threshold(&self) -> Option<u64>; // Threshold in seconds
    fn get_task_order(&self) -> usize;
}


pub fn create_request_builder(client: &Client, api_config: &ApiConfig) -> Result<RequestBuilder, String> {
    let mut headers = HeaderMap::new();
    for (key, value) in &api_config.headers {
        match (HeaderName::from_str(key), HeaderValue::from_str(value)) {
            (Ok(header_name), Ok(header_value)) => {
                headers.insert(header_name, header_value);
            },
            _ => return Err(format!("Invalid header: {}: {}", key, value)),
        }
    }

    let body_content = if let Some(body_file_path) = &api_config.body_file {
        fs::read_to_string(body_file_path)
            .map_err(|e| format!("Error reading request body from file '{}': {}", body_file_path, e))?
    } else {
        api_config.body.clone().unwrap_or_default()
    };

    let request_builder = match &api_config.method {
        HttpMethod::POST => Ok(client.post(&api_config.url).headers(headers).body(body_content)),
        HttpMethod::PUT => Ok(client.put(&api_config.url).headers(headers).body(body_content)),
        HttpMethod::DELETE => Ok(client.delete(&api_config.url).headers(headers)),
        HttpMethod::GET => Ok(client.get(&api_config.url).headers(headers)),
        // Extend this match to handle other HTTP methods as needed
    };

    request_builder
}

pub fn create_monitor_tasks(cfg: &Settings, app_state: Arc<Mutex<AppState>>) -> VecDeque<Box<dyn ApiMonitor + Send + Sync>> {
    let mut tasks: VecDeque<Box<dyn ApiMonitor + Send + Sync>> = VecDeque::new();

    for api_config in cfg.apis.iter() {
        // Use the task's name in logging
        if api_config.load_test.unwrap_or(false) {
            if let Some(load_test_config) = &api_config.load_test_config {
                info!("Configuring progressive load test '{}'", api_config.name); // Changed from url to name
                tasks.push_back(Box::new(LoadTest {
                    api_config: Arc::new(api_config.clone()),
                    app_state: app_state.clone(),
                    load_test_config: load_test_config.clone(),
                }));
            }
        } else {
            info!("Configuring task '{}'", api_config.name); // Log task configuration with name
            tasks.push_back(Box::new(Task {
                api_config: Arc::new(api_config.clone()),
                app_state: app_state.clone(),
            }));
        }
    }

    tasks
}



pub async fn start_monitoring(cfg: Arc<Settings>, app_state: Arc<Mutex<AppState>>) {
    let http_config = HttpClientConfig {
        timeout_seconds: cfg.http_timeout_seconds,
        proxy_url: cfg.http_proxy_url.clone(),
        default_headers: cfg.http_default_headers.clone(),
    };

    let client = http_client::get_client(Some(http_config)).expect("Failed to create HTTP client");

    // Create tasks based on the configuration
    let tasks = create_monitor_tasks(&cfg, app_state);

    // Instead of trying to access `task_order` directly from `cfg.apis`, which is incorrect,
    // you should leverage the tasks created by `create_monitor_tasks` function.
    // This assumes that each task holds a reference to its configuration, including `task_order`.

    // Group tasks by `task_order`. This requires that your task implementations
    // provide access to their respective `ApiConfig` (or at least the `task_order`).
    let mut grouped_tasks: std::collections::HashMap<usize, Vec<Box<dyn ApiMonitor + Send + Sync>>> = std::collections::HashMap::new();
    for task in tasks {
        // Assuming each task type has a method to access its `task_order`
        let order = task.get_task_order(); // Corrected line
        grouped_tasks.entry(order).or_insert_with(Vec::new).push(task);
    }

    // Sort group keys to ensure tasks are executed in the correct order
    let mut order_keys: Vec<&usize> = grouped_tasks.keys().collect();
    order_keys.sort();

    // Execute task groups in sorted order
    for order_key in order_keys {
        if let Some(task_group) = grouped_tasks.get(order_key) {
            let futures: Vec<_> = task_group.iter().map(|task| {
                let client_clone = client.clone();
                async move {
                    info!("Starting '{}'", task.describe()); // Log the start of a task using its name
                    match task.execute(&client_clone).await {
                        Ok(_) => info!("Successfully completed '{}'", task.describe()), // Log successful completion
                        Err(e) => log::error!("Task '{}' failed: {}", task.describe(), e), // Log failure with task name
                    }
                }
            }).collect();

            join_all(futures).await; // Execute concurrently within the same order group
        }
    }
}

