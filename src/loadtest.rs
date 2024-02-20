
use serde::{Serialize, Deserialize};
use futures::future::join_all;
use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{Mutex, Semaphore};
use std::time::Duration;
use tokio::time::Instant;

use crate::{appstate::AppState, config::{ApiConfig, HttpMethod, LoadTestConfig}, factory::{create_request_builder, ApiMonitor}};


/// Monitors and executes load tests for a specific API endpoint.
///
/// This struct is responsible for conducting load tests based on configurations
/// specified in `ApiConfig` and `LoadTestConfig`. It utilizes an HTTP client
/// for making requests and records the results in the shared application state.
#[derive(Debug, Clone)]
pub struct LoadTest {
    /// Configuration for the API endpoint to be tested.
    pub api_config: Arc<ApiConfig>,
    /// A reference to the shared application state where test results are recorded.
    pub app_state: Arc<Mutex<AppState>>,
    /// Configuration specifying the parameters of the load test.
    pub load_test_config: LoadTestConfig,
}

/// Represents the aggregated results of a load test.
///
/// This struct captures various metrics collected during the execution of a load test,
/// including counts of successful and failed requests, response time statistics,
/// and distribution of response status codes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadTestMonitoringData {
    /// The total number of requests made during the load test.
    pub total_requests: usize,
    /// The number of successful requests.
    pub success_count: usize,
    /// The number of failed requests.
    pub failure_count: usize,
    /// The median response time in milliseconds.
    pub median_response_time_ms: u128,
    /// The average response time in milliseconds.
    pub average_response_time_ms: u128,
    /// The minimum response time in milliseconds observed during the test.
    pub min_response_time_ms: u128,
    /// The maximum response time in milliseconds observed during the test.
    pub max_response_time_ms: u128,
    /// A distribution of response status codes received.
    pub status_code_distribution: HashMap<u16, usize>,
    /// The 95th percentile response time in milliseconds.
    pub percentile_95th_response_time_ms: u128,
    /// The rate of requests per second.
    pub requests_per_second: f64,
    /// The average size of the response in bytes (placeholder for actual data collection).
    pub average_bytes_per_response: u128,
    /// The HTTP method used in the load test.
    pub method: HttpMethod,
}


#[async_trait]
impl ApiMonitor for LoadTest {

    /// Executes the load test, making multiple requests to the target API endpoint
    /// according to the `LoadTestConfig` settings, and records the results.
    ///
    /// # Parameters
    /// - `client`: The HTTP client used to make requests.
    ///
    /// # Returns
    /// A `Result` indicating the success or failure of the load test execution.
    async fn execute(&self, client: &Client) -> Result<(), String> {
        let mut attempt = 0;
        let max_attempts = self.load_test_config.retry_count.unwrap_or(0) as usize; // Provide a default value if `retry_count` is None and cast to usize for comparison

        while attempt <= max_attempts {
            match self.run_load_test(client).await {
                Ok(_) => return Ok(()),
                Err(e) if attempt < max_attempts => { // Correct comparison with unwrapped and converted retry_count
                    log::warn!("Load test attempt {} failed: {}. Retrying...", attempt + 1, e);
                    attempt += 1;
                    tokio::time::sleep(Duration::from_secs(5)).await; // Backoff before retry
                },
                Err(e) => return Err(format!("Load test failed after {} attempts: {}", attempt + 1, e)),
            }
        }

        Err("Load test failed: Maximum retry attempts reached".to_string())
    }

    /// Provides a descriptive name for the load test, incorporating the API endpoint's name
    /// for easier identification.
    ///
    /// # Returns
    /// A string containing the descriptive name of the load test.
    fn describe(&self) -> String {
        format!("LoadTest for {}", self.api_config.name)
    }

    /// Optionally specifies a response time threshold for the load test.
    ///
    /// # Returns
    /// An optional `u64` representing the response time threshold in milliseconds, if applicable.
    fn response_time_threshold(&self) -> Option<u64> {
        None
    }

    /// Retrieves the order in which the load test should be executed relative to other tasks.
    ///
    /// # Returns
    /// A `usize` representing the load test's execution order.
    fn get_task_order(&self) -> usize {
        self.api_config.task_order.unwrap_or(usize::MAX)
    }
}

impl LoadTest {

     /// Asynchronously executes the load test against the configured API endpoint.
    ///
    /// This method simulates concurrent users by spawning asynchronous tasks that
    /// send requests to the API endpoint, respecting the configuration parameters
    /// such as initial load, maximum load, spawn rate, and maximum duration of the test.
    ///
    /// # Parameters
    /// - `client`: The HTTP client used to send requests to the API.
    ///
    /// # Returns
    /// A `Result<(), String>` indicating the success or failure of the load test.
    /// On success, it returns `Ok(())`. On failure, it returns an `Err` with an error message.
    async fn run_load_test(&self, client: &Client) -> Result<(), String> {
        // Records the start time of the load test to calculate the total duration later.
        let start_time = Instant::now();

        // Initializes a vector to store results of each load test step.
        let mut all_results = Vec::new();

        // Sets a sensible default for max_duration if not specified, here assumed as 1 second for simplicity.
        let sensible_max_duration_secs: u64 = 1;
        // Retrieves max_duration from the test configuration, using the sensible default if not specified.
        let max_duration_secs = self.load_test_config.max_duration_secs
                                    .map(|secs| secs as u64)
                                    .unwrap_or(sensible_max_duration_secs);
        // Converts the duration from seconds to a Duration object for easier comparison.
        let max_duration = Duration::from_secs(max_duration_secs);

        // Initializes the current load based on the test configuration or defaults to 0.
        let mut current_load = self.load_test_config.initial_load.unwrap_or_default();
        // Retrieves the maximum load from the configuration or uses the maximum usize value if not specified.
        let max_load = self.load_test_config.max_load.unwrap_or(usize::MAX);
        // Retrieves the spawn rate (users per second) from the configuration, defaulting to 1 if not specified.
        let spawn_rate = self.load_test_config.spawn_rate.unwrap_or(1);

        // Sets up a repeating interval of 1 second to control the spawn rate.
        let mut interval = tokio::time::interval(Duration::from_secs(1));

        // Continues to execute the load test until the current load reaches the max load or the max duration is exceeded.
        while current_load < max_load && start_time.elapsed() < max_duration {
            // Waits for the next tick of the interval, effectively pausing for 1 second.
            interval.tick().await;

            // Calculates the number of new users to spawn this tick, without exceeding the max load.
            let new_users = std::cmp::min(spawn_rate, max_load - current_load);
            // Updates the current load by adding the new users.
            current_load += new_users;

            // Logs the number of new users being spawned and the total current load.
            log::info!("Spawning {} new users, total users: {}", new_users, current_load);

            // Creates a semaphore with a number of permits equal to the current load, controlling concurrent access.
            let semaphore = Arc::new(Semaphore::new(current_load));

            // Maps each new user to a spawned task, creating a vector of these tasks.
            let tasks = (0..new_users).map(|_| {
                // Clones the client and API configuration for use within the async task.
                let client_clone = client.clone();
                let api_config_clone = self.api_config.clone();
                let semaphore_clone = semaphore.clone();

                // Spawns an asynchronous task for each user.
                tokio::spawn(async move {
                    // Acquires a permit from the semaphore before proceeding, ensuring concurrency control.
                    let _permit = semaphore_clone.acquire_owned().await.expect("Failed to acquire semaphore permit");
                    // Records the start time of the request for duration calculation.
                    let start = Instant::now();

                    // Attempts to create a request builder using the client and API configuration.
                    let request_result = create_request_builder(&client_clone, &api_config_clone);
                    match request_result {
                        // If successful, sends the request and awaits the response.
                        Ok(request_builder) => {
                            let response = request_builder.send().await;
                            match response {
                                // On successful response, extracts the status code, response body, and calculates the duration.
                                Ok(resp) => {
                                    let status = resp.status();
                                    let body = resp.text().await.unwrap_or_default();
                                    let bytes = body.len();
                                    let duration = start.elapsed();
                                    // Returns the status code, duration, and response size.
                                    Ok((status, duration, bytes))
                                },
                                // Logs any errors encountered while sending the request.
                                Err(e) => {
                                    log::error!("Request error: {}", e);
                                    Err(e.to_string())
                                },
                            }
                        },
                        // Logs any errors encountered while creating the request builder.
                        Err(e) => {
                            log::error!("Request creation error: {}", e);
                            Err(e)
                        },
                    }
                })
            }).collect::<Vec<_>>();


            let join_results = join_all(tasks).await;
            let step_results = join_results.into_iter().map(|join_result| {
                join_result.unwrap_or_else(|join_error| {
                    log::error!("Task panicked: {:?}", join_error);
                    Err("Task panicked".to_string())
                })
            }).collect::<Vec<_>>();

            all_results.extend(step_results);

            if start_time.elapsed() >= max_duration {
                log::info!("Max duration reached, ending load test early.");
                break;
            }
        }

        // Once the load test loop is complete, calculate the total duration
        let total_duration = start_time.elapsed();
        log::info!("Load test completed. Total duration: {:?}", total_duration);

        // Filter the results to only include successful requests and calculate statistics.
        let filtered_results: Vec<(StatusCode, Duration, usize)> = all_results.into_iter()
            .filter_map(|result| match result {
                Ok((status, duration, bytes)) => Some((status, duration, bytes)),
                Err(_) => None,
            })
            .collect();

        // Analyze the filtered results to compute summary statistics.
        let (success_count,
            failure_count,
            median_response_time_ms,
            average_response_time_ms,
            min_response_time_ms,
            max_response_time_ms,
            status_code_distribution,
            percentile_95th_response_time_ms,
            requests_per_second,
            average_bytes_per_response) = analyze_results(&filtered_results);

        // Construct LoadTestMonitoringData
        let load_test_data = LoadTestMonitoringData {
            total_requests: filtered_results.len(),
            success_count,
            failure_count,
            median_response_time_ms,
            average_response_time_ms,
            min_response_time_ms,
            max_response_time_ms,
            status_code_distribution,
            percentile_95th_response_time_ms,
            requests_per_second,
            average_bytes_per_response,
            method: self.api_config.method.clone(),
        };

        // Update application state with load test data
        update_load_test_app_state(&self.app_state, &self.api_config.url, load_test_data).await;

        Ok(())
    }
}


/// Analyzes the results of a load test to calculate various performance metrics.
///
/// This function processes an array of results from load test requests to compute statistics such as
/// success and failure counts, average and median response times, response time percentiles,
/// status code distributions, requests per second, and average bytes per response.
///
/// # Parameters
/// - `results`: A slice of tuples containing the status code, duration, and size in bytes
///   of each request made during the load test.
///
/// # Returns
/// A tuple containing the following aggregated metrics:
/// - `usize`: The total number of successful requests.
/// - `usize`: The total number of failed requests.
/// - `u128`: The median response time in milliseconds.
/// - `u128`: The average response time in milliseconds.
/// - `u128`: The minimum response time in milliseconds observed in the test.
/// - `u128`: The maximum response time in milliseconds observed in the test.
/// - `HashMap<u16, usize>`: A distribution of HTTP status codes received during the test.
/// - `u128`: The 95th percentile response time in milliseconds.
/// - `f64`: The rate of requests per second calculated from the test duration and total requests.
/// - `u128`: The average size in bytes of the responses received.
///
/// The function ensures that all metrics are calculated accurately to provide a comprehensive
/// overview of the load test's performance.
fn analyze_results(results: &[(StatusCode, Duration, usize)]) -> (usize, usize, u128, u128, u128, u128, HashMap<u16, usize>, u128, f64, u128) {
    let mut success_count = 0;
    let mut failure_count = 0;
    let mut total_duration = 0u128;
    let mut total_bytes = 0u128; // Accumulator for total bytes
    let mut response_times_ms = Vec::new(); // Collect all response times for percentile calculation
    let mut min_response_time_ms = u128::MAX;
    let mut max_response_time_ms = u128::MIN;
    let mut status_code_distribution = HashMap::new();

    for (status, duration, bytes) in results {
        if status.is_success() {
            success_count += 1;
        } else {
            failure_count += 1;
        }

        let duration_ms = duration.as_millis();
        response_times_ms.push(duration_ms);
        total_duration += duration_ms;
        total_bytes += *bytes as u128; // Add the response size to the total
        min_response_time_ms = min_response_time_ms.min(duration_ms);
        max_response_time_ms = max_response_time_ms.max(duration_ms);

        *status_code_distribution.entry(status.as_u16()).or_insert(0) += 1;
    }

    let average_response_time_ms = if !results.is_empty() {
        total_duration / results.len() as u128
    } else {
        0
    };

    // Calculate the 95th percentile
    response_times_ms.sort_unstable();
    let percentile_95th_index = (0.95 * (response_times_ms.len() as f64)).ceil() as usize - 1;
    let percentile_95th_response_time_ms = *response_times_ms.get(percentile_95th_index).unwrap_or(&0);

    // Calculate Median
    let median_response_time_ms = if response_times_ms.len() % 2 == 0 {
        let mid_right = response_times_ms.len() / 2;
        let mid_left = mid_right - 1;
        (response_times_ms[mid_left] + response_times_ms[mid_right]) / 2
    } else {
        response_times_ms[response_times_ms.len() / 2]
    };

    // Calculate Requests per Second (RPS)
    let total_test_duration_secs = total_duration as f64 / 1000.0; // Convert milliseconds to seconds
    let requests_per_second = if total_test_duration_secs > 0.0 {
        results.len() as f64 / total_test_duration_secs
    } else {
        0.0
    };

    // Calculate Average Bytes per Response
    let average_bytes_per_response = if !results.is_empty() {
        total_bytes / results.len() as u128
    } else {
        0
    };

    (
        success_count,
        failure_count,
        median_response_time_ms,
        average_response_time_ms,
        min_response_time_ms,
        max_response_time_ms,
        status_code_distribution,
        percentile_95th_response_time_ms,
        requests_per_second,
        average_bytes_per_response
    )
}

/// Updates the shared application state with the results of a load test.
///
/// # Parameters
/// - `app_state`: A reference to the shared application state.
/// - `api_url`: The URL of the API that was load tested.
/// - `load_test_data`: The aggregated data collected from the load test.
async fn update_load_test_app_state(app_state: &Arc<Mutex<AppState>>, api_url: &str, load_test_data: LoadTestMonitoringData) {
    let state = app_state.lock().await;
    let mut data = state.load_test_monitoring_data.lock().await;
    data.insert(api_url.to_string(), load_test_data);

    // Log the update for debugging or informational purposes
    log::info!("Updated load test data for {}", api_url);
}
