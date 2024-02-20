pub mod appstate;
pub mod config;
pub mod utils;
pub mod factory;
pub mod loadtest;
pub mod tasks;
pub mod cli;

use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use cli::process_http_default_headers;
use config::{load_workflow, Settings, Workflow};
use factory::start_monitoring;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;
use crate::appstate::AppState;
use crate::cli::build_cli;



// Entry point for the Actix web server.
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Parse command line arguments using clap.
    let matches = build_cli().get_matches();

    // Extract configuration file or directory from CLI arguments.
    let config_file = matches.get_one::<String>("config").map(|s| s.to_string());
    let config_dir = matches.get_one::<String>("config-dir").map(|s| s.to_string());

    // Load workflows based on provided configuration.
    let workflows = load_workflow(config_file, config_dir).await.expect("Failed to load workflows");

    // Extract optional HTTP proxy URL from CLI arguments.
    let http_proxy_url = matches.get_one::<String>("http_proxy_url").map(|s| s.to_string());

    // Process and validate HTTP default headers specified in CLI arguments.
    let http_default_headers = process_http_default_headers(&matches)
        .unwrap_or_else(|err| {
            eprintln!("Error processing HTTP default headers: {}", err);
            std::process::exit(1);
        });

    // Initialize application settings based on CLI arguments.
    let global_settings = Settings {
        monitoring_interval_seconds: matches.get_one::<String>("monitoring_interval_seconds")
            .and_then(|s| s.parse().ok())
            .unwrap_or(60), // Default to 60 seconds if not specified
        log_level: matches.get_one::<String>("log_level")
            .unwrap_or(&"info".to_string()).clone(), // Default to "info" if not specified
        http_timeout_seconds: matches.get_one::<String>("http_timeout_seconds")
            .and_then(|s| s.parse().ok())
            .unwrap_or(20), // Default to 20 seconds if not specified
        http_proxy_url,
        http_default_headers,
    };

    // Initialize logging based on the specified log level.
    global_settings.init_logging();

    // Wrap workflows and settings in Arcs for thread-safe shared access across async tasks.
    let workflows_arc = Arc::new(workflows.into_iter().map(Arc::new).collect::<Vec<_>>());
    let settings_arc = Arc::new(global_settings);

    // Prepare the shared application state for concurrent access.
    let app_state_arc = Arc::new(Mutex::new(AppState {
        load_test_monitoring_data: Arc::new(Mutex::new(HashMap::new())),
        task_monitoring_data: Arc::new(Mutex::new(HashMap::new())),
    }));

    // Make shared state accessible in Actix web handlers through web::Data.
    let app_state_for_actix = web::Data::new(app_state_arc.clone());
    let workflows_for_actix = web::Data::new(workflows_arc.clone());
    let settings_for_actix = web::Data::new(settings_arc.clone());

    // Launch a background task for monitoring based on the current configuration.
    let workflows_vec = Arc::clone(&workflows_arc);
    let app_state_clone = app_state_arc.clone();
    let settings_clone = settings_arc.clone();

    tokio::spawn(async move {
        start_monitoring(settings_clone, (*workflows_vec).clone(), app_state_clone).await;
    });

    // Set up and run the Actix web server with configured routes and handlers.
    HttpServer::new(move || {
        App::new()
            .app_data(app_state_for_actix.clone())
            .app_data(settings_for_actix.clone())
            .app_data(workflows_for_actix.clone())
            .route("/load_test_results", web::get().to(get_load_test_data))
            .route("/trigger_load_tests", web::get().to(trigger_monitoring))
            .route("/task_results", web::get().to(get_task_data))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}


// Handles web requests to retrieve load test data, utilizing shared application state.
async fn get_load_test_data(
    data: web::Data<Arc<Mutex<AppState>>> // Provides thread-safe access to the AppState.
) -> impl Responder {
    // Asynchronously acquires a lock on AppState to access its contents safely.
    let app_state = data.lock().await;

    // Locks the load test monitoring data for safe access.
    let load_test_data = app_state.load_test_monitoring_data.lock().await;

    // Serializes and responds with the load test data in JSON format.
    HttpResponse::Ok().json(&*load_test_data)
}

// Asynchronously triggers monitoring based on the provided settings, app state, and workflows.
async fn trigger_monitoring(
    settings: web::Data<Arc<Settings>>,
    app_state: web::Data<Arc<Mutex<AppState>>>,
    workflows: web::Data<Arc<Vec<Arc<Workflow>>>>,
) -> impl actix_web::Responder {
    // Clones the settings, app state, and workflows to pass to the monitoring task.
    let settings_clone = Arc::clone(settings.get_ref());
    let app_state_clone = Arc::clone(app_state.get_ref());
    let workflows_clone = Arc::clone(workflows.get_ref());

    // Spawns an asynchronous task to start monitoring with the cloned arguments.
    tokio::spawn(async move {
        start_monitoring(settings_clone, (*workflows_clone).clone(), app_state_clone).await;
    });

    // Responds to indicate that load test monitoring has been triggered.
    HttpResponse::Ok().body("Load test triggered.")
}

// Retrieves and responds with HTTP status data from the shared application state.
async fn get_task_data(data: web::Data<Arc<Mutex<AppState>>>) -> impl actix_web::Responder {
    // Safely accesses the application state and its HTTP status data.
    let app_state = data.lock().await;
    let http_status_data = app_state.task_monitoring_data.lock().await;

    // Serializes and responds with the HTTP status data in JSON format.
    HttpResponse::Ok().json(&*http_status_data)
}
