use reqwest::{Client, Error, header::HeaderMap, header::HeaderName, header::HeaderValue};
use std::collections::HashMap;
use std::time::Duration;
use std::str::FromStr;

pub struct HttpClientConfig {
    pub timeout_seconds: u64,
    pub proxy_url: Option<String>,
    pub default_headers: HashMap<String, String>,
}

impl Default for HttpClientConfig {
    fn default() -> Self {
        Self {
            timeout_seconds: 30, // Default timeout of 30 seconds
            proxy_url: None, // No proxy by default
            default_headers: HashMap::new(), // No default headers
        }
    }
}

pub fn get_client(config: Option<HttpClientConfig>) -> Result<Client, Error> {
    let config = config.unwrap_or_default();

    let mut client_builder = Client::builder()
        .timeout(Duration::from_secs(config.timeout_seconds));

    // Configure proxy if specified
    if let Some(proxy_url) = config.proxy_url {
        if let Ok(proxy) = reqwest::Proxy::all(&proxy_url) {
            client_builder = client_builder.proxy(proxy);
        } else {
            eprintln!("Invalid proxy URL: {}", proxy_url);
        }
    }

    // Initialize an empty HeaderMap
    let mut headers = HeaderMap::new();

    // Add default headers if specified
    for (key, value) in config.default_headers.iter() {
        // Convert each key and value to HeaderName and HeaderValue
        if let (Ok(h_key), Ok(h_value)) = (HeaderName::from_str(key), HeaderValue::from_str(value)) {
            headers.insert(h_key, h_value);
        } else {
            eprintln!("Invalid header: {}: {}", key, value);
        }
    }

    client_builder = client_builder.default_headers(headers);

    client_builder.build()
}
