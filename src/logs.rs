use colored::*;
use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

static LOG_REGEX: OnceLock<Regex> = OnceLock::new();
static KV_REGEX: OnceLock<Regex> = OnceLock::new();
static REQ_CTX_REGEX: OnceLock<Regex> = OnceLock::new();

pub fn highlight_line(line: &str) -> String {
    let re = LOG_REGEX.get_or_init(|| {
        // Matches standard tracing output:
        // 2024-03-20T10:00:00.123456Z  INFO request{...}: target: message
        // or
        // 2024-03-20T10:00:00.123456Z  INFO target: message
        Regex::new(r"^([\d\-:T\.Z]+)\s+([A-Z]+)\s+(?:(request\{.*?\})[:\s]+)?(.*?):\s+(.*)$")
            .unwrap()
    });

    if let Some(caps) = re.captures(line) {
        let timestamp = caps.get(1).map_or("", |m| m.as_str());
        let level = caps.get(2).map_or("", |m| m.as_str());
        let request_context = caps.get(3).map_or("", |m| m.as_str());
        let target = caps.get(4).map_or("", |m| m.as_str());
        let message = caps.get(5).map_or("", |m| m.as_str());

        let colored_level = match level {
            "ERROR" => level.red().bold(),
            "WARN" => level.yellow().bold(),
            "INFO" => level.green(),
            "DEBUG" => level.blue(),
            "TRACE" => level.magenta(),
            _ => level.normal(),
        };

        let request_display = if !request_context.is_empty() {
            format_request_context(request_context)
        } else {
            String::new()
        };

        format!(
            "{} {} {}{}: {}",
            timestamp.dimmed(),
            colored_level,
            request_display,
            target.cyan(),
            highlight_message(message)
        )
    } else {
        highlight_keywords(line)
    }
}

fn format_request_context(context: &str) -> String {
    let re = REQ_CTX_REGEX.get_or_init(|| Regex::new(r"(\w+)=([^\s,}}]+)").unwrap());

    let mut parts = HashMap::new();
    for cap in re.captures_iter(context) {
        if let (Some(key), Some(val)) = (cap.get(1), cap.get(2)) {
            parts.insert(key.as_str(), val.as_str());
        }
    }

    let mut output = String::new();

    // Request ID (Shortened)
    if let Some(req_id) = parts.get("request_id") {
        let short_id = if req_id.len() > 8 {
            &req_id[..8]
        } else {
            req_id
        };
        output.push_str(&format!("[{}] ", short_id.yellow()));
    }

    // Client IP
    if let Some(ip) = parts
        .get("client_ip")
        .filter(|ip| !ip.is_empty() && **ip != "unknown")
    {
        output.push_str(&format!("[{}] ", ip.purple()));
    }

    // Other fields (team_id, router_name, etc.) are available in the span
    // but not included in the prefix to keep logs clean and consistent.
    // They are logged as explicit events (e.g. "Team Resolved: ...").

    output
}

fn highlight_message(message: &str) -> String {
    let mut result = message.to_string();

    // Highlight HTTP methods
    let get_colored = "GET".green().to_string();
    result = result.replace("GET", &get_colored);
    result = result.replace("POST", &"POST".blue().to_string());
    result = result.replace("PUT", &"PUT".yellow().to_string());
    result = result.replace("DELETE", &"DELETE".red().to_string());

    // Highlight key=value pairs
    let kv_re = KV_REGEX.get_or_init(|| Regex::new(r"(\w+)=([^\s,]+)").unwrap());

    result = kv_re
        .replace_all(&result, |caps: &regex::Captures| {
            let key = &caps[1];
            let val = &caps[2];
            format!("{}={}", key.purple(), val.cyan())
        })
        .to_string();

    result
}

fn highlight_keywords(line: &str) -> String {
    let mut result = line.to_string();

    if result.contains("ERROR") {
        result = result.replace("ERROR", &"ERROR".red().bold().to_string());
    } else if result.contains("WARN") {
        result = result.replace("WARN", &"WARN".yellow().bold().to_string());
    } else if result.contains("INFO") {
        result = result.replace("INFO", &"INFO".green().to_string());
    } else if result.contains("DEBUG") {
        result = result.replace("DEBUG", &"DEBUG".blue().to_string());
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_highlight_line_with_request_context() {
        colored::control::set_override(true);
        let line = "2024-03-20T10:00:00.123Z INFO request{request_id=1234567890 team_id=team_1 client_ip=127.0.0.1 uri=/v1/chat}: apex::server: Listening";
        let colored = highlight_line(line);

        println!("Colored output: {}", colored);

        assert!(colored.contains("12345678")); // Shortened ID
        assert!(colored.contains("127.0.0.1"));
        assert!(!colored.contains("request{")); // Context should be hidden/formatted
    }

    #[test]
    fn test_highlight_line_standard() {
        colored::control::set_override(true);
        let line = "2024-03-20T10:00:00.123Z INFO apex::server: Listening on port 8080";
        let colored = highlight_line(line);
        // Verify structure is preserved
        assert!(colored.contains("2024-03-20T10:00:00.123Z"));
        assert!(colored.contains("apex::server"));
        assert!(colored.contains("Listening on port 8080"));

        // Verify coloring
        // INFO should be green
        assert!(colored.contains("\x1b[32mINFO\x1b[0m"));
    }

    #[test]
    fn test_highlight_key_value() {
        colored::control::set_override(true);
        let line =
            "2024-03-20T10:00:00.123Z INFO apex::request: method=GET path=/v1/chat status=200";
        let colored = highlight_line(line);

        // Check if purple color is applied to status
        assert!(colored.contains("\x1b[35mstatus"));
        // Check if cyan color is applied to 200
        assert!(colored.contains("\x1b[36m200"));
    }
}
