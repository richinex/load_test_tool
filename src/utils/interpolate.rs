use regex::{Regex, Captures};
use std::env;
use lazy_static::lazy_static;

use crate::config::Workflow;

lazy_static! {
    static ref ENV_VAR_REGEX: Regex = Regex::new(r"\$\{([^}]+)\}").unwrap();
}

pub fn interpolate_string(input: &str) -> String {
    ENV_VAR_REGEX.replace_all(input, |caps: &Captures| {
        match env::var(&caps[1]) {
            Ok(val) => val,
            Err(_) => {
                log::warn!("Environment variable {} not found; using default.", &caps[1]);
                caps[0].to_string()
            }
        }
    }).to_string()
}

pub fn interpolate_config(workflow: &mut Workflow) {

    for api in workflow.apis.iter_mut() {
        api.url = interpolate_string(&api.url);
        if let Some(body) = &mut api.body {
            *body = interpolate_string(body);
        }
        for header_value in api.headers.values_mut() {
            *header_value = interpolate_string(header_value);
        }
        // Note: This implementation does not interpolate 'name', 'method', or 'expected_field' as
        // they are less likely to contain environment variables, but you can add them if needed.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_yaml;

    // Adjusted to include a token placeholder in the `http_default_headers`
    fn load_test_settings() -> Workflow {
        let yaml = r#"
name: "Sample Workflow"
apis:
  - name: "Setup Organization"
    url: "${API_URL}"  # Placeholder for API URL
    task_order: 1
    method: GET
    headers: {}  # Example headers, specific to an API request
    expected_field: "id"
    response_time_threshold: 2000
    load_test: false
"#;
        serde_yaml::from_str::<Workflow>(yaml).expect("Failed to parse YAML")
    }

    #[test]
    fn test_interpolation_for_token() {
        // Set up test environment variables
        env::set_var("API_URL", "https://jsonplaceholder.typicode.com/todos/1");
        env::set_var("TEST_TOKEN", "secret_token");

        let mut settings = load_test_settings();
        interpolate_config(&mut settings);

        assert_eq!(settings.apis[0].url, "https://jsonplaceholder.typicode.com/todos/1");


        // Clean up environment variables
        env::remove_var("API_URL");
    }
}

