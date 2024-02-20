use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::loadtest::LoadTestMonitoringData;
use crate::tasks::MonitoringData;

#[derive(Debug)]
pub struct AppState {
    pub load_test_monitoring_data: Arc<Mutex<HashMap<String, LoadTestMonitoringData>>>,
    pub task_monitoring_data: Arc<Mutex<HashMap<String, MonitoringData>>>,
}
