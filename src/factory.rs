use log::info;

use futures::future::join_all;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::config::{Settings, Workflow};
use crate::appstate::AppState;
use crate::loadtest::LoadTest;
use crate::tasks::Task;
use crate::utils::http_client::{self, HttpClientConfig};
use std::{fs, str::FromStr};
use reqwest::{Client, RequestBuilder};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use crate::config::{ApiConfig, HttpMethod};
use reqwest::Client as HttpClient;



#[async_trait::async_trait]
pub trait ApiMonitor {
    async fn execute(&self, client: &reqwest::Client, workflow_name: &str) -> Result<(), String>;
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

pub fn create_monitor_tasks(cfg: &Workflow, app_state: Arc<Mutex<AppState>>) -> VecDeque<Box<dyn ApiMonitor + Send + Sync>> {
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


async fn monitor_single_workflow(workflow: Arc<Workflow>, app_state: Arc<Mutex<AppState>>, client: HttpClient) {
    let workflow_name = &workflow.name;
    let tasks = create_monitor_tasks(&workflow, app_state);

    let mut grouped_tasks: HashMap<usize, Vec<Box<dyn ApiMonitor + Send + Sync>>> = HashMap::new();
    for task in tasks {
        let order = task.get_task_order(); // Assume this exists and is correct
        grouped_tasks.entry(order).or_insert_with(Vec::new).push(task);
    }

    let mut order_keys: Vec<&usize> = grouped_tasks.keys().collect();
    order_keys.sort();

    for order_key in order_keys {
        if let Some(task_group) = grouped_tasks.get(order_key) {
            let futures: Vec<_> = task_group.iter().map(|task| {
                let client_clone = client.clone();
                async move {
                    info!("Starting '{}'", task.describe());
                    match task.execute(&client_clone, workflow_name).await {
                        Ok(_) => info!("Successfully completed '{}'", task.describe()),
                        Err(e) => log::error!("Task '{}' failed: {}", task.describe(), e),
                    }
                }
            }).collect();

            join_all(futures).await; // Execute concurrently within the same order group
        }
    }
}



// Updated function signature to accept a vector of workflows
pub async fn start_monitoring(settings: Arc<Settings>, workflows: Vec<Arc<Workflow>>, app_state: Arc<Mutex<AppState>>) {
    let http_config = HttpClientConfig {
        timeout_seconds: settings.http_timeout_seconds,
        proxy_url: settings.http_proxy_url.clone(),
        default_headers: settings.http_default_headers.clone(),
    };

    let client = http_client::get_client(Some(http_config)).expect("Failed to create HTTP client");

    // Iterate over workflows and spawn a new async task for each
    let futures: Vec<_> = workflows.into_iter().map(|workflow| {
        let app_state_clone = app_state.clone();
        let client_clone = client.clone();
        monitor_single_workflow(workflow, app_state_clone, client_clone)
    }).collect();

    // Wait for all spawned tasks to complete
    join_all(futures).await;
}
