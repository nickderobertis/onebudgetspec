//! A real JSON reporter subprocess used by the saved-report journeys.
use onebudgetspec_core::delivery::{
    DeliveryResult, PublishRequest, merge_standing_report, validate_request,
};
use std::io::{Read, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = String::new();
    std::io::stdin().read_to_string(&mut bytes)?;
    let request: PublishRequest = serde_json::from_str(&bytes)?;
    validate_request(&request)?;
    std::fs::write("publish-request.json", &bytes)?;
    match request.key.as_str() {
        "exit" => std::process::exit(7),
        "denied" => {
            std::io::stdout().write_all(
                b"{\"delivered\":false,\"diagnostic\":\"reporter credential unavailable\"}\n",
            )?;
            return Ok(());
        }
        "malformed" => {
            std::io::stdout().write_all(b"not JSON\n")?;
            return Ok(());
        }
        _ => {}
    }
    let previous = std::fs::read_to_string("standing.md").ok();
    let body = merge_standing_report(previous.as_deref(), &request, 4096);
    std::fs::write("standing.md", body)?;
    let result = DeliveryResult {
        delivered: true,
        url: Some(request.destination),
        diagnostic: None,
    };
    std::io::stdout().write_all(&serde_json::to_vec(&result)?)?;
    Ok(())
}
