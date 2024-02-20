use std::{collections::HashMap, str::FromStr, sync::Arc};
use log::{info,error};
use tokio::sync::Mutex;
use reqwest::Client;
use serde::Serialize;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use crate::{appstate::AppState, config::{ApiConfig, HttpMethod}, factory::{create_request_builder, ApiMonitor}};
use std::time::Instant;


/// Represents the data collected during the monitoring of an API call.
#[derive(Debug, Clone, Serialize)]
pub struct MonitoringData {
    /// The name of the workflow this data is associated with.
    pub api_url: String,
    /// The status of the monitoring operation, e.g., "OK" or "ERROR".
    pub status: String,
    /// The response time measured for the API call, in milliseconds.
    pub response_time: u64,
    /// The HTTP status code returned by the API call, if applicable.
    pub status_code: Option<u16>,
    /// The HTTP method used for the API call.
    pub method: HttpMethod,
}


pub enum MonitoringDataType {
    /// Represents a simple task monitoring operation.
    Task,
}


pub struct Task {
    /// Configuration for the API call to be monitored.
    pub api_config: Arc<ApiConfig>,
    /// A reference to the shared application state for recording monitoring data.
    pub app_state: Arc<Mutex<AppState>>, // Include a reference to AppState
}

#[async_trait::async_trait]
impl ApiMonitor for Task {

    async fn execute(&self, client: &Client, workflow_name: &str) -> Result<(), String> {
        let start = Instant::now();
        let mut headers = HeaderMap::new();

        for (key, value) in &self.api_config.headers {
            match (HeaderName::from_str(key), HeaderValue::from_str(value)) {
                (Ok(header_name), Ok(header_value)) => {
                    headers.insert(header_name, header_value);
                },
                _ => continue, // Skip invalid headers
            }
        }

        let request_builder = create_request_builder(client, &self.api_config)?;

        let response = request_builder.send().await;

        let duration = start.elapsed();

        // Create a MonitoringData instance based on the response
        match response {
            Ok(resp) => {
                let status_code = resp.status().as_u16();
                if resp.status().is_success() {
                    // If the status is within the range of success codes
                    let monitoring_data = MonitoringData {
                        api_url: self.api_config.url.clone(),
                        status: "OK".to_string(),
                        response_time: duration.as_millis() as u64,
                        status_code: Some(status_code), // Store the successful status code
                        method: self.api_config.method.clone(), // Include the method in the monitoring data
                    };
                    update_app_state(&self.app_state, &workflow_name, &self.api_config.name, MonitoringDataType::Task, monitoring_data).await;
                    info!("'{}' succeeded with status code {} in {:?}", self.api_config.name, status_code, duration);
                    Ok(())
                } else {
                    // For non-successful HTTP status codes
                    let error_message = format!("'{}' responded with HTTP status {}", self.api_config.name, status_code);
                    error!("{}", error_message);
                    let monitoring_data = MonitoringData {
                        api_url: self.api_config.url.clone(),
                        status: "ERROR".to_string(),
                        response_time: duration.as_millis() as u64,
                        status_code: Some(status_code), // Store the error status code
                        method: self.api_config.method.clone(), // Include the method in the monitoring data
                    };
                    update_app_state(&self.app_state, &workflow_name, &self.api_config.name, MonitoringDataType::Task, monitoring_data).await;
                    Err(error_message)
                }
            },
            Err(e) => {
                // Error handling remains similar, but now without a status code
                let error_message = format!("Failed to reach '{}': {}", self.api_config.name, e);
                error!("{}", &error_message);
                let monitoring_data = MonitoringData {
                    api_url: self.api_config.url.clone(),
                    status: "ERROR".to_string(),
                    response_time: duration.as_millis() as u64,
                    status_code: None, // No status code available in case of a connection error
                    method: self.api_config.method.clone(), // Include the method in the monitoring data
                };
                update_app_state(&self.app_state, &workflow_name,  &self.api_config.name, MonitoringDataType::Task, monitoring_data).await;
                Err(error_message)
            }
        }
    }

    fn describe(&self) -> String {
        format!("Task for {}", self.api_config.name)
    }

    fn response_time_threshold(&self) -> Option<u64> {
        None // No specific threshold for HTTP status monitoring
    }

    fn get_task_order(&self) -> usize {
        self.api_config.task_order.unwrap_or(usize::MAX)
    }
}


async fn update_app_state(
    app_state: &Arc<Mutex<AppState>>,
    workflow_name: &str,
    task_name: &str,
    data_type: MonitoringDataType,
    monitoring_data: MonitoringData
) {
    // Lock the Mutex to access the underlying HashMap
    let state = app_state.lock().await;

    // Decide which part of the state to update based on the data type
    match data_type {
        MonitoringDataType::Task => {
            // Ensure we have a mutable reference to the HashMap
            let task_monitoring_data = &mut *state.task_monitoring_data.lock().await;

            // Access or create the nested HashMap for the specified workflow
            let workflow_data = task_monitoring_data
                .entry(workflow_name.to_string()) // Now correctly using entry on the HashMap
                .or_insert_with(HashMap::new);

            // Update the monitoring data for the specific API URL within the workflow
            workflow_data.insert(task_name.to_string(), monitoring_data);

            log::info!("Updated task data for {} in workflow {}", task_name, workflow_name);
        },
        // Handle other data types as needed
    };
}
