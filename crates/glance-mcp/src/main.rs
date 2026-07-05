use std::io::{self, BufRead, Write};

use serde_json::Value;

fn main() {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else {
            break;
        };
        if line.trim().is_empty() {
            continue;
        }

        let request = match serde_json::from_str::<Value>(&line) {
            Ok(value) => value,
            Err(err) => {
                eprintln!("glance-mcp: invalid json: {err}");
                continue;
            }
        };

        if let Some(response) = glance_mcp::handle_json_rpc(&request)
            && let Ok(line) = serde_json::to_string(&response)
        {
            let _ = writeln!(stdout, "{line}");
            let _ = stdout.flush();
        }
    }
}
