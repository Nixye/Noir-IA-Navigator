fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|a| a == "--sandbox-probe") {
        return security_sandbox::run_probe(args.get(2).map(String::as_str).unwrap_or(""));
    }
    if args.get(1).is_some_and(|a| a == "--sandbox-probe-child") {
        return Ok(());
    }
    security_sandbox::ensure_verified(&std::env::current_exe().map_err(|e| e.to_string())?)?;
    println!(
        "AppContainer verified: token, zero capabilities, files, network and child-process denial"
    );
    Ok(())
}
