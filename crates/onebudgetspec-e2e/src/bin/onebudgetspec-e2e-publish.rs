//! A real JSON reporter subprocess used by the saved-report journeys.
use onebudgetspec_core::delivery::{DeliveryResult, PublishRequest, merge_standing_report};
use std::io::{Read, Write};

fn main() {
    let mut bytes = String::new();
    std::io::stdin().read_to_string(&mut bytes).unwrap();
    let request: PublishRequest = serde_json::from_str(&bytes).unwrap();
    std::fs::write("publish-request.json", &bytes).unwrap();
    match request.key.as_str() {
        "exit" => std::process::exit(7),
        "denied" => {
            println!("{{\"delivered\":false,\"diagnostic\":\"reporter credential unavailable\"}}");
            return;
        }
        "malformed" => {
            println!("not JSON");
            return;
        }
        _ => {}
    }
    let previous = std::fs::read_to_string("standing.md").ok();
    let body = merge_standing_report(previous.as_deref(), &request, 4096);
    std::fs::write("standing.md", body).unwrap();
    let result = DeliveryResult {
        delivered: true,
        url: Some(request.destination),
        diagnostic: None,
    };
    std::io::stdout()
        .write_all(&serde_json::to_vec(&result).unwrap())
        .unwrap();
}
