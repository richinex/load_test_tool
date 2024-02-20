pub mod appstate;
pub mod config;
pub mod utils;
pub mod factory;
pub mod loadtest;
pub mod tasks;

use actix_web::{web, App, HttpResponse, HttpServer};
use config::{load_config, Settings};
use factory::start_monitoring;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;
use crate::appstate::AppState;



// The entry point of the Actix web server.
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Load the application configuration asynchronously and initialize logging.
    // `load_config` is an async function that loads settings from a configuration source (e.g., a file or environment variables).
    let settings = load_config().await.expect("Failed to load configuration");
    settings.init_logging(); // Initialize logging as configured in `settings`.

    // Wrap the loaded settings in an `Arc` for thread-safe reference counting.
    // This allows the settings to be shared across multiple parts of the application without copying them.
    let settings_arc = Arc::new(settings);

    // Create the shared application state, including a mutex-protected hash map for load test monitoring data.
    // Wrapping this state in an `Arc` and `Mutex` ensures thread-safe mutable access across async tasks.
    let app_state_arc = Arc::new(Mutex::new(AppState {
        load_test_monitoring_data: Arc::new(Mutex::new(HashMap::new())),
        task_monitoring_data: Arc::new(Mutex::new(HashMap::new())),
    }));

    // Wrap the shared application state (`app_state_arc`) and settings (`settings_arc`) in `web::Data` for Actix.
    // `web::Data` provides an efficient way to access shared data within request handlers.
    let app_state_for_actix = web::Data::new(app_state_arc.clone());
    let settings_for_actix = web::Data::new(settings_arc.clone());

    // Spawn a background task to start monitoring.
    // This task uses cloned references to the settings and application state to avoid ownership issues.
    let settings_clone = settings_arc.clone();
    let app_state_clone = app_state_arc.clone();
    tokio::spawn(async move {
        start_monitoring(settings_clone, app_state_clone).await;
    });

    // Configure and run the Actix web server.
    // This includes setting up shared application data and defining route handlers.
    HttpServer::new(move || {
        App::new()
            // Add the shared application state and settings to the app's data.
            // This makes them accessible in route handlers.
            .app_data(app_state_for_actix.clone()) // Shared state accessible in handlers.
            .app_data(settings_for_actix.clone()) // Shared settings accessible in handlers.

            // Define routes and their handlers.
            // `/load_test_data` returns serialized load test monitoring data.
            // `/trigger_load_tests` triggers the load testing process.
            .route("/load_test_data", web::get().to(get_load_test_data))
            .route("/trigger_load_tests", web::get().to(trigger_monitoring))
            .route("/http_status_data", web::get().to(get_task_data))
    })
    // Bind the server to an IP address and port.
    .bind("127.0.0.1:8080")?
    // Start the server asynchronously.
    .run()
    .await
}



// An asynchronous function designed to handle web requests to retrieve load test data.
// It takes the shared application state as a parameter.
async fn get_load_test_data(
    // `data`: The shared application state necessary for accessing load test data,
    // wrapped in `web::Data` for Actix integration and `Arc<Mutex<AppState>>` for thread-safe access.
    data: web::Data<Arc<Mutex<AppState>>>
) -> impl actix_web::Responder {

    // Lock the `app_state` asynchronously to safely access its contents. This prevents data races
    // when multiple threads attempt to access `app_state` concurrently.
    let app_state = data.lock().await;

    // Once the lock is acquired, access the `load_test_monitoring_data` within `app_state`.
    // This also requires a lock because it's wrapped in a `Mutex`, ensuring safe access to the
    // mutable load test data.
    let load_test_data = app_state.load_test_monitoring_data.lock().await;

    // Serialize the load test data to JSON and send it as the response.
    // The `&*` operator is used to dereference the smart pointer (`MutexGuard`) to access
    // the underlying data directly for serialization.
    HttpResponse::Ok().json(&*load_test_data)
}

// Define an asynchronous function named `trigger_monitoring` that will be used as a route handler.
// This function is designed to be called from an HTTP route in an Actix Web server.
async fn trigger_monitoring(
    // The first parameter, `settings`, is of type `web::Data<Arc<Settings>>`.
    // `web::Data` is a wrapper used by Actix Web to safely share data between handlers.
    // `Arc<Settings>` is an atomic reference-counted pointer to `Settings`, allowing for thread-safe,
    // shared access to the application's configuration settings.
    settings: web::Data<Arc<Settings>>, // Use web::Data to pass Arc<Settings>

    // The second parameter, `app_state`, is of type `web::Data<Arc<Mutex<AppState>>>`.
    // Similar to `settings`, `app_state` uses `web::Data` for sharing across handlers.
    // `Arc<Mutex<AppState>>` provides thread-safe, mutable access to the application state.
    // `Mutex` ensures that concurrent access to `AppState` is properly synchronized.
    app_state: web::Data<Arc<Mutex<AppState>>> // Use web::Data to pass Arc<Mutex<AppState>>
) -> impl actix_web::Responder { // The function returns a type that implements the `Responder` trait, allowing it to respond to HTTP requests.

    // Inside the function body, `tokio::spawn` is used to execute `start_monitoring` asynchronously.
    // This allows the load test to run concurrently without blocking the current handler or other parts of the application.
    tokio::spawn(async move { // `tokio::spawn` takes an async block, moving ownership of captured variables into the block.
        // `settings.get_ref().clone()` retrieves a reference to the `Arc<Settings>` inside `web::Data` and clones it.
        // Cloning an `Arc` is cheap and simply increments the reference count.
        // `app_state.get_ref().clone()` does the same for `Arc<Mutex<AppState>>`, providing a cloned `Arc` to the `start_monitoring` function.
        // The `start_monitoring` function is then called with these cloned `Arc` references, initiating the load testing process.
        start_monitoring(settings.get_ref().clone(), app_state.get_ref().clone()).await;
    });

    // After spawning the load test task, the function immediately responds with an HTTP 200 OK status,
    // and a body message indicating that the load test has been triggered.
    // This response is sent back to the client without waiting for the load test to complete,
    // making the operation effectively non-blocking from the client's perspective.
    HttpResponse::Ok().body("Load test triggered.")
}

async fn get_task_data(data: web::Data<Arc<Mutex<AppState>>>) -> impl actix_web::Responder {
    let app_state = data.lock().await;
    let http_status_data = app_state.task_monitoring_data.lock().await;

    HttpResponse::Ok().json(&*http_status_data) // Serialize the HTTP status monitoring data
}